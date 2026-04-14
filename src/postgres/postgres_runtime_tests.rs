use super::{
    ArtifactRefInsert, ChunkRecord, ContextPackInsert, DocumentRecord, ImportPacketUpdate,
    MemoryCardVerificationConflictCheck, MemoryConflictInsert, MemoryEdgeInsert, MemoryItemInsert,
    MemoryItemRecord, MemoryLinkDecisionInsert, MemoryProvenanceInsert, NamespaceRecord,
    ObservabilityInsertMeta, PendingLinkProposalInsert, PolicyRuleInsert, ProjectRecord,
    QuarantineItemInsert, RelationUpdate, RestorePackInsert, RetrievalTraceInsert, SkillCardRecord,
    SkillCardVerificationConflictCheck, SymbolRecord, TaskEventInsert, TaskNodeCandidateExtraction,
    TaskNodeInsert, TaskNodeVerificationConflictCheck, add_relation, apply_memory_card_update,
    augment_memory_item_metadata_with_stage2_runtime, bind_shared_asset_to_project,
    build_memory_write_pipeline, build_skill_execution_cards, canonical_repo_root_string,
    connect_admin, count_documents_for_project_namespace_codes, create_artifact_ref,
    create_import_packet, create_memory_card, create_memory_conflict, create_memory_edge,
    create_memory_item, create_memory_link_decision, create_memory_provenance,
    create_memory_relation_edge, create_pending_link_proposal, create_policy_rule,
    create_quarantine_item, create_restore_pack, create_retrieval_trace,
    create_skill_card_candidate, create_skill_evidence_bundle, create_task_event, create_task_node,
    derive_memory_item_source_kind, ensure_access_policy, ensure_namespace, ensure_shared_asset,
    ensure_transfer_policy, ensure_workspace, evidence_span_marks_skill_card_poisoned,
    exact_match_basename, exact_match_basename_stem, extract_memory_card_candidate,
    extract_memory_item_candidate, extract_skill_card_candidate, extract_task_node_candidate,
    get_import_packet, get_namespace_by_code, get_project_by_code, get_stack_meta, get_task_node,
    insert_artifact_ref, insert_context_pack, insert_observability_snapshot, list_skill_cards,
    memory_item_has_recorded_basis, memory_write_async_index_subjects,
    memory_write_fan_out_subjects, metadata_marks_memory_item_poisoned,
    observability_conflict_error, observability_source_class, prepare_observability_payload,
    provenance_marks_memory_card_poisoned, reconcile_import_packet_quarantines, record_skill_eval,
    record_skill_reuse_log, record_skill_trial_run, record_skill_trigger_match,
    replace_document_index, run_memory_card_policy_scope_filter,
    run_memory_item_policy_scope_filter, run_skill_card_policy_scope_filter,
    run_task_node_policy_scope_filter, safe_postgres_descriptor, search_memory_cards_for_namespace,
    task_node_marks_poisoned, update_import_packet, update_memory_card_truth_state,
    update_relation, upsert_project, upsert_stack_meta, validate_artifact_ref_basis,
    validate_memory_card_candidate, validate_memory_card_policy_scope_filter,
    validate_memory_card_runtime_states, validate_memory_card_verification_conflict_check,
    validate_memory_item_candidate, validate_memory_item_policy_scope_filter,
    validate_memory_item_verification_conflict_check, validate_memory_link_decision_basis,
    validate_memory_relation_edge_basis, validate_observability_update,
    validate_pending_link_proposal_basis, validate_skill_activity_basis,
    validate_skill_card_candidate, validate_skill_card_policy_scope_filter,
    validate_skill_card_verification_conflict_check, validate_skill_evidence_bundle_basis,
    validate_stage2_basis, validate_task_event_basis, validate_task_node_candidate,
    validate_task_node_policy_scope_filter, validate_task_node_verification_conflict_check,
};
use crate::config::AppConfig;
use crate::nats;
use serde_json::{Value, json};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio_postgres::Client;
use uuid::Uuid;

async fn ensure_project_alpha_test_namespace(
    client: &Client,
    namespace_code: &str,
) -> NamespaceRecord {
    let project = super::get_project_by_code(client, "project_alpha")
        .await
        .expect("project_alpha");
    ensure_namespace(
        client,
        project.project_id,
        namespace_code,
        Some(namespace_code),
        "local_strict",
    )
    .await
    .expect("test namespace")
}

#[test]
fn write_pipeline_materializes_stage_two_contract() {
    let memory_item = MemoryItemRecord {
        memory_item_id: Uuid::parse_str("00000000-0000-0000-0000-000000000111").expect("uuid"),
        workspace_code: "default".to_string(),
        project_code: "project_alpha".to_string(),
        namespace_code: Some("review".to_string()),
        source_project_code: None,
        import_packet_id: None,
        owner_agent_id: Some(
            Uuid::parse_str("00000000-0000-0000-0000-000000000222").expect("uuid"),
        ),
        visibility_scope: "project_shared".to_string(),
        item_kind: "fact".to_string(),
        identity_key: Some("stage2-proof".to_string()),
        title: "stage2 proof item".to_string(),
        summary: Some("summary".to_string()),
        body: Some("body".to_string()),
        sensitivity_class: "confidential".to_string(),
        truth_state: "current".to_string(),
        trust_state: "verified".to_string(),
        verification_state: "verified".to_string(),
        lifecycle_state: "hot".to_string(),
        source_event_ids: json!(["event:stage2"]),
        artifact_refs: json!(["artifact://proof/stage2"]),
        message_refs: json!(["message:stage2"]),
        evidence_span: json!({"path":"fixtures/project_alpha/src/lib.rs","line_start":1,"line_end":3}),
        derivation_kind: "extract".to_string(),
        observed_at_epoch_ms: Some(1000),
        recorded_at_epoch_ms: Some(1005),
        valid_from_epoch_ms: Some(1000),
        valid_to_epoch_ms: Some(2000),
        last_verified_at_epoch_ms: Some(1500),
        object_version: 2,
        ingest_seq: 7,
        causation_id: Some("cause-stage2".to_string()),
        correlation_id: Some("corr-stage2".to_string()),
        utility_score: 0.9,
        freshness_score: 0.8,
        retention_class: "durable".to_string(),
        ttl_epoch_ms: Some(60000),
        access_count: 0,
        last_accessed_at: None,
        decay_policy: "standard".to_string(),
        consolidation_status: "active".to_string(),
        imported_from: json!({"source":"proof","kind":"local"}),
        schema_version: "memory-envelope-v1".to_string(),
        superseded_by_memory_item_id: None,
        metadata: json!({"proof":"stage2"}),
    };
    let candidate = super::MemoryItemCandidateExtraction {
        source_basis_status: "recorded".to_string(),
        source_event_count: 1,
        artifact_ref_count: 1,
        message_ref_count: 1,
        has_evidence_span: true,
        source_kind: Some("raw_event_append".to_string()),
        imported_from: json!({"source":"proof","kind":"local"}),
        raw_event_kind: "memory_candidate_write".to_string(),
        raw_event_payload: json!({"candidate":{"item_kind":"fact"}}),
        candidate_class: "fact".to_string(),
        hot_path_write_eligible: false,
        background_consolidation_recommended: true,
    };
    let scope_filter = super::MemoryItemPolicyScopeFilter {
        visibility_scope: "project_shared".to_string(),
        sensitivity_class: "confidential".to_string(),
        workspace_code: "default".to_string(),
        project_code: "project_alpha".to_string(),
        namespace_code: Some("review".to_string()),
        owner_agent_required: false,
        owner_agent_present: true,
        private_contour_violation: false,
        quarantine_contour_violation: false,
        cross_project_basis_present: false,
        source_project_bound: false,
        import_packet_present: false,
        import_packet_found: false,
        import_packet_source_matches: true,
        import_packet_target_matches: true,
        import_packet_status: None,
        controlled_transfer_required: false,
        controlled_transfer_valid: true,
        scope_allowed: true,
    };
    let verification_check = super::MemoryItemVerificationConflictCheck {
        evidence_present: true,
        current_truth_conflict: false,
        poisoned_detected: false,
        private_contour_violation: false,
        truth_state: "current".to_string(),
        trust_state: "verified".to_string(),
        verification_state: "verified".to_string(),
        superseded_by_memory_item_id: None,
        write_allowed: true,
    };

    let pipeline = build_memory_write_pipeline(
        &memory_item,
        &candidate,
        &scope_filter,
        &verification_check,
        Uuid::parse_str("00000000-0000-0000-0000-000000000333").expect("uuid"),
        &memory_write_async_index_subjects(),
        &memory_write_fan_out_subjects(),
    );

    assert_eq!(
        pipeline["contract_version"].as_str(),
        Some("memory-write-pipeline-v1")
    );
    assert_eq!(
        pipeline["raw_event_append"]["status"].as_str(),
        Some("written")
    );
    assert_eq!(
        pipeline["raw_event_append"]["source_basis_status"].as_str(),
        Some("recorded")
    );
    assert_eq!(
        pipeline["raw_event_append"]["source_event_count"].as_u64(),
        Some(1)
    );
    assert_eq!(
        pipeline["memory_candidate_extraction"]["item_kind"].as_str(),
        Some("fact")
    );
    assert_eq!(
        pipeline["memory_candidate_extraction"]["derivation_kind"].as_str(),
        Some("extract")
    );
    assert_eq!(
        pipeline["memory_candidate_extraction"]["candidate_class"].as_str(),
        Some("fact")
    );
    assert_eq!(
        pipeline["memory_candidate_extraction"]["hot_path_write_eligible"].as_bool(),
        Some(false)
    );
    assert_eq!(
        pipeline["memory_candidate_extraction"]["background_consolidation_recommended"].as_bool(),
        Some(true)
    );
    assert_eq!(
        pipeline["policy_and_scope_filter"]["visibility_scope"].as_str(),
        Some("project_shared")
    );
    assert_eq!(
        pipeline["policy_and_scope_filter"]["project_code"].as_str(),
        Some("project_alpha")
    );
    assert_eq!(
        pipeline["policy_and_scope_filter"]["scope_allowed"].as_bool(),
        Some(true)
    );
    assert_eq!(
        pipeline["verification_conflict_check"]["verification_state"].as_str(),
        Some("verified")
    );
    assert_eq!(
        pipeline["verification_conflict_check"]["evidence_present"].as_bool(),
        Some(true)
    );
    assert_eq!(
        pipeline["verification_conflict_check"]["write_allowed"].as_bool(),
        Some(true)
    );
    assert_eq!(
        pipeline["truth_write"]["storage_lane"].as_str(),
        Some("ami.memory_items")
    );
    assert_eq!(pipeline["truth_write"]["object_version"].as_i64(), Some(2));
    assert_eq!(pipeline["truth_write"]["ingest_seq"].as_i64(), Some(7));
    assert_eq!(
        pipeline["async_indexing"]["status"].as_str(),
        Some("queued")
    );
    assert_eq!(
        pipeline["cache_invalidation_fan_out"]["status"].as_str(),
        Some("queued")
    );
}

#[test]
fn memory_item_source_kind_prefers_explicit_then_detected_basis() {
    let source_event_ids = vec!["event:stage2".to_string()];
    let artifact_refs = vec!["artifact://proof/stage2".to_string()];
    let message_refs = vec!["message:stage2".to_string()];
    let explicit_metadata = json!({"source_kind":"explicit_source_kind"});
    let evidence_span = json!({"path":"fixtures/project_alpha/src/lib.rs","line_start":1});
    let explicit = MemoryItemInsert {
        source_project_code: None,
        import_packet_id: None,
        owner_agent_code: None,
        item_kind: "fact",
        identity_key: Some("stage2-proof"),
        title: "stage2 proof item",
        summary: Some("summary"),
        body: Some("body"),
        sensitivity_class: Some("confidential"),
        truth_state: Some("current"),
        trust_state: Some("verified"),
        verification_state: Some("verified"),
        lifecycle_state: Some("hot"),
        source_event_ids: &source_event_ids,
        artifact_refs: &artifact_refs,
        message_refs: &message_refs,
        evidence_span: &evidence_span,
        derivation_kind: Some("extract"),
        observed_at_epoch_ms: Some(1000),
        recorded_at_epoch_ms: Some(1005),
        valid_from_epoch_ms: Some(1000),
        valid_to_epoch_ms: Some(2000),
        last_verified_at_epoch_ms: Some(1500),
        object_version: Some(2),
        causation_id: Some("cause-stage2"),
        correlation_id: Some("corr-stage2"),
        utility_score: Some(0.9),
        freshness_score: Some(0.8),
        retention_class: Some("durable"),
        ttl_epoch_ms: Some(60000),
        decay_policy: None,
        consolidation_status: None,
        imported_from: None,
        schema_version: Some("memory-envelope-v1"),
        superseded_by_memory_item_id: None,
        metadata: &explicit_metadata,
    };
    assert_eq!(
        derive_memory_item_source_kind(&explicit).as_deref(),
        Some("explicit_source_kind")
    );
    assert!(memory_item_has_recorded_basis(&explicit));

    let imported_metadata = json!({});
    let imported = MemoryItemInsert {
        source_project_code: Some("project_beta"),
        import_packet_id: Some(
            Uuid::parse_str("00000000-0000-0000-0000-000000000333").expect("uuid"),
        ),
        metadata: &imported_metadata,
        ..explicit.clone()
    };
    assert_eq!(
        derive_memory_item_source_kind(&imported).as_deref(),
        Some("import_packet_basis")
    );

    let empty: Vec<String> = Vec::new();
    let empty_span = json!({});
    let operator_only = MemoryItemInsert {
        source_project_code: None,
        import_packet_id: None,
        source_event_ids: &empty,
        artifact_refs: &empty,
        message_refs: &empty,
        evidence_span: &empty_span,
        metadata: &imported_metadata,
        ..explicit
    };
    assert_eq!(derive_memory_item_source_kind(&operator_only), None);
    assert!(!memory_item_has_recorded_basis(&operator_only));
}

#[test]
fn memory_candidate_extraction_marks_background_semantic_consolidation() {
    let source_event_ids = vec!["event:stage2".to_string()];
    let artifact_refs = vec!["artifact://proof/stage2".to_string()];
    let message_refs = vec!["message:stage2".to_string()];
    let evidence_span = json!({"path":"fixtures/project_alpha/src/lib.rs","line_start":1});
    let metadata = json!({"source_kind":"proof_contract"});
    let record = MemoryItemInsert {
        source_project_code: None,
        import_packet_id: None,
        owner_agent_code: None,
        item_kind: "fact",
        identity_key: Some("stage2-proof"),
        title: "stage2 proof item",
        summary: Some("summary"),
        body: Some("body"),
        sensitivity_class: Some("confidential"),
        truth_state: Some("current"),
        trust_state: Some("verified"),
        verification_state: Some("verified"),
        lifecycle_state: Some("hot"),
        source_event_ids: &source_event_ids,
        artifact_refs: &artifact_refs,
        message_refs: &message_refs,
        evidence_span: &evidence_span,
        derivation_kind: Some("extract"),
        observed_at_epoch_ms: Some(1000),
        recorded_at_epoch_ms: Some(1005),
        valid_from_epoch_ms: Some(1000),
        valid_to_epoch_ms: Some(2000),
        last_verified_at_epoch_ms: Some(1500),
        object_version: Some(2),
        causation_id: Some("cause-stage2"),
        correlation_id: Some("corr-stage2"),
        utility_score: Some(0.9),
        freshness_score: Some(0.8),
        retention_class: Some("durable"),
        ttl_epoch_ms: Some(60000),
        decay_policy: None,
        consolidation_status: None,
        imported_from: None,
        schema_version: Some("memory-envelope-v1"),
        superseded_by_memory_item_id: None,
        metadata: &metadata,
    };
    let workspace_id =
        Uuid::parse_str("00000000-0000-0000-0000-000000000001").expect("workspace uuid");
    let project = ProjectRecord {
        project_id: Uuid::parse_str("00000000-0000-0000-0000-000000000002").expect("project uuid"),
        code: "project_alpha".to_string(),
        display_name: "Project Alpha".to_string(),
        repo_root: "/tmp/project_alpha".to_string(),
        visibility_scope: "project_shared".to_string(),
        updated_at: "2026-01-01T00:00:00Z".to_string(),
    };
    let namespace = NamespaceRecord {
        namespace_id: Uuid::parse_str("00000000-0000-0000-0000-000000000003")
            .expect("namespace uuid"),
        code: "review".to_string(),
        display_name: "Review".to_string(),
        retrieval_mode: "hybrid".to_string(),
    };
    let source_event_json = json!(source_event_ids);
    let artifact_refs_json = json!(artifact_refs);
    let message_refs_json = json!(message_refs);
    let candidate = extract_memory_item_candidate(
        workspace_id,
        &project,
        &namespace,
        None,
        None,
        &record,
        record.observed_at_epoch_ms,
        record.recorded_at_epoch_ms,
        record.valid_from_epoch_ms,
        record.valid_to_epoch_ms,
        &source_event_json,
        &artifact_refs_json,
        &message_refs_json,
    );
    assert_eq!(candidate.candidate_class, "fact");
    assert_eq!(candidate.source_basis_status, "recorded");
    assert_eq!(candidate.source_kind.as_deref(), Some("proof_contract"));
    assert!(!candidate.hot_path_write_eligible);
    assert!(candidate.background_consolidation_recommended);
    assert_eq!(
        candidate.raw_event_payload["runtime_lane"]["background_consolidation_recommended"]
            .as_bool(),
        Some(true)
    );
}

#[test]
fn memory_policy_scope_filter_requires_owner_for_agent_private() {
    let project = ProjectRecord {
        project_id: Uuid::parse_str("00000000-0000-0000-0000-000000000100").expect("project uuid"),
        code: "project_alpha".to_string(),
        display_name: "Project Alpha".to_string(),
        repo_root: "/tmp/project_alpha".to_string(),
        visibility_scope: "agent_private".to_string(),
        updated_at: "2026-01-01T00:00:00Z".to_string(),
    };
    let namespace = NamespaceRecord {
        namespace_id: Uuid::parse_str("00000000-0000-0000-0000-000000000101")
            .expect("namespace uuid"),
        code: "review".to_string(),
        display_name: "Review".to_string(),
        retrieval_mode: "hybrid".to_string(),
    };
    let record = MemoryItemInsert {
        source_project_code: None,
        import_packet_id: None,
        owner_agent_code: None,
        item_kind: "fact",
        identity_key: None,
        title: "private contour item",
        summary: None,
        body: None,
        sensitivity_class: Some("internal"),
        truth_state: Some("current"),
        trust_state: Some("verified"),
        verification_state: Some("verified"),
        lifecycle_state: Some("hot"),
        source_event_ids: &[],
        artifact_refs: &[],
        message_refs: &[],
        evidence_span: &json!({}),
        derivation_kind: Some("operator_write"),
        observed_at_epoch_ms: None,
        recorded_at_epoch_ms: None,
        valid_from_epoch_ms: None,
        valid_to_epoch_ms: None,
        last_verified_at_epoch_ms: None,
        object_version: None,
        causation_id: None,
        correlation_id: None,
        utility_score: None,
        freshness_score: None,
        retention_class: None,
        ttl_epoch_ms: None,
        decay_policy: None,
        consolidation_status: None,
        imported_from: None,
        schema_version: None,
        superseded_by_memory_item_id: None,
        metadata: &json!({}),
    };
    let filter =
        run_memory_item_policy_scope_filter("default", &project, &namespace, None, &record);
    assert!(filter.owner_agent_required);
    assert!(!filter.owner_agent_present);
    assert!(filter.private_contour_violation);
    assert!(!filter.scope_allowed);
    let error = validate_memory_item_policy_scope_filter(&filter).expect_err("must fail");
    assert!(error.to_string().contains("requires owner_agent binding"));
}

#[test]
fn memory_policy_scope_filter_rejects_quarantine_visibility() {
    let filter = super::MemoryItemPolicyScopeFilter {
        visibility_scope: "quarantine".to_string(),
        sensitivity_class: "internal".to_string(),
        workspace_code: "default".to_string(),
        project_code: "project_quarantine".to_string(),
        namespace_code: Some("review".to_string()),
        owner_agent_required: false,
        owner_agent_present: false,
        private_contour_violation: false,
        quarantine_contour_violation: true,
        cross_project_basis_present: false,
        source_project_bound: false,
        import_packet_present: false,
        import_packet_found: false,
        import_packet_source_matches: true,
        import_packet_target_matches: true,
        import_packet_status: None,
        controlled_transfer_required: false,
        controlled_transfer_valid: true,
        scope_allowed: false,
    };
    let error = validate_memory_item_policy_scope_filter(&filter).expect_err("must fail");
    assert!(error.to_string().contains("dedicated quarantine_item path"));
}

#[test]
fn memory_verification_conflict_check_detects_poison_metadata() {
    assert!(metadata_marks_memory_item_poisoned(
        &json!({"poisoned": true})
    ));
    assert!(metadata_marks_memory_item_poisoned(
        &json!({"safety": {"poisoned": true}})
    ));
    assert!(!metadata_marks_memory_item_poisoned(
        &json!({"proof": "stage2"})
    ));
    let check = super::MemoryItemVerificationConflictCheck {
        evidence_present: true,
        current_truth_conflict: false,
        poisoned_detected: true,
        private_contour_violation: false,
        truth_state: "current".to_string(),
        trust_state: "verified".to_string(),
        verification_state: "verified".to_string(),
        superseded_by_memory_item_id: None,
        write_allowed: false,
    };
    let error = validate_memory_item_verification_conflict_check(&check).expect_err("must fail");
    assert!(error.to_string().contains("flagged poisoned"));
}

#[test]
fn memory_card_candidate_extraction_marks_runtime_contract() {
    let tags = vec!["decision".to_string()];
    let provenance = json!({
        "source_event_ids": ["event:memory-card"],
        "artifact_refs": ["artifact://proof/card"],
        "message_refs": ["message:memory-card"],
        "evidence_span": {"path":"docs/proof.md","line_start":1,"line_end":2}
    });
    let candidate = extract_memory_card_candidate(
        "Operator decision card",
        &tags,
        &provenance,
        Some("subject"),
        Some("predicate"),
        Some("object"),
    );
    assert_eq!(candidate.source_basis_status, "recorded");
    assert_eq!(candidate.source_event_count, 1);
    assert_eq!(candidate.artifact_ref_count, 1);
    assert_eq!(candidate.message_ref_count, 1);
    assert!(candidate.has_evidence_span);
    assert_eq!(candidate.derivation_kind, "extract");
    assert_eq!(candidate.candidate_class, "decision");
    assert_eq!(candidate.source_kind.as_deref(), Some("raw_event_append"));
    assert!(candidate.hot_path_write_eligible);
    assert!(!candidate.background_consolidation_recommended);
}

#[test]
fn memory_card_candidate_validation_rejects_basis_free_extract() {
    let provenance = json!({});
    let candidate =
        extract_memory_card_candidate("plain fact card", &[], &provenance, None, None, None);
    let error = validate_memory_card_candidate(&candidate).expect_err("basis-free card rejected");
    assert!(error
            .to_string()
            .contains("memory card candidate requires recorded provenance basis unless derivation_kind=operator_write"));
}

#[test]
fn memory_card_policy_scope_filter_requires_owner_for_agent_private() {
    let project = ProjectRecord {
        project_id: Uuid::nil(),
        code: "project_private".to_string(),
        display_name: "project_private".to_string(),
        repo_root: "/tmp/project_private".to_string(),
        visibility_scope: "agent_private".to_string(),
        updated_at: "2026-01-01T00:00:00Z".to_string(),
    };
    let namespace = NamespaceRecord {
        namespace_id: Uuid::nil(),
        code: "review".to_string(),
        display_name: "review".to_string(),
        retrieval_mode: "hybrid".to_string(),
    };
    let filter = run_memory_card_policy_scope_filter(&project, &namespace, &json!({}));
    assert!(filter.owner_agent_required);
    assert!(!filter.owner_agent_present);
    assert!(filter.private_contour_violation);
    let error = validate_memory_card_policy_scope_filter(&filter).expect_err("must fail closed");
    assert!(
        error
            .to_string()
            .contains("requires owner_agent binding in provenance")
    );
}

#[test]
fn memory_card_policy_scope_filter_rejects_quarantine_visibility() {
    let filter = super::MemoryCardPolicyScopeFilter {
        visibility_scope: "quarantine".to_string(),
        sensitivity_class: "internal".to_string(),
        project_code: "project_quarantine".to_string(),
        namespace_code: "review".to_string(),
        owner_agent_required: false,
        owner_agent_present: false,
        private_contour_violation: false,
        scope_allowed: false,
    };
    let error = validate_memory_card_policy_scope_filter(&filter).expect_err("must fail closed");
    assert!(error.to_string().contains("dedicated quarantine_item path"));
}

#[test]
fn memory_card_verification_conflict_check_detects_poisoned_provenance() {
    let provenance = json!({
        "poisoned": true,
        "source_event_ids": ["event:poisoned-memory-card"]
    });
    assert!(provenance_marks_memory_card_poisoned(&provenance));
    let check = MemoryCardVerificationConflictCheck {
        evidence_present: true,
        current_truth_conflict: false,
        poisoned_detected: true,
        private_contour_violation: false,
        truth_state: "current".to_string(),
        verification_state: "verified".to_string(),
        status: "active".to_string(),
        write_allowed: false,
    };
    let error = validate_memory_card_verification_conflict_check(&check).expect_err("must fail");
    assert!(error.to_string().contains("flagged poisoned"));
}

#[test]
fn memory_card_runtime_state_validation_rejects_invalid_truth_state() {
    let error = validate_memory_card_runtime_states(
        Some("stale"),
        Some("verified"),
        Some("active"),
        "memory apply-card-update",
    )
    .expect_err("invalid truth_state rejected before sql");
    assert!(
        error
            .to_string()
            .contains("invalid memory card truth_state 'stale' for memory apply-card-update")
    );
}

#[test]
fn memory_card_runtime_state_validation_accepts_schema_allowed_values() {
    validate_memory_card_runtime_states(
        Some("conflicted"),
        Some("disputed"),
        Some("archived"),
        "memory update-card-truth-state",
    )
    .expect("schema-aligned states accepted");
    validate_memory_card_runtime_states(
        Some("unverified"),
        Some("proposed"),
        Some("active"),
        "memory create-card",
    )
    .expect("schema-aligned create-card states accepted");
}

#[test]
fn task_node_policy_scope_filter_requires_owner_for_agent_private() {
    let project = ProjectRecord {
        project_id: Uuid::nil(),
        code: "project_private".to_string(),
        display_name: "project_private".to_string(),
        repo_root: "/tmp/project_private".to_string(),
        visibility_scope: "agent_private".to_string(),
        updated_at: "2026-01-01T00:00:00Z".to_string(),
    };
    let namespace = NamespaceRecord {
        namespace_id: Uuid::nil(),
        code: "review".to_string(),
        display_name: "review".to_string(),
        retrieval_mode: "hybrid".to_string(),
    };
    let record = TaskNodeInsert {
        parent_task_node_id: None,
        memory_item_id: None,
        task_key: Some("task-private"),
        task_role: Some("workline"),
        headline: "Private task",
        summary: Some("summary"),
        next_step: Some("next"),
        execution_state: Some("proposed"),
        lifecycle_state: Some("hot"),
        confidence: None,
        current_score: None,
        reopened_count: None,
        child_count: None,
        closed_child_count: None,
        pending_return_count: None,
        source_event_ids: Some(&json!(["event:task-private"])),
        artifact_refs: Some(&json!(["artifact://proof/task-private"])),
        evidence_span: Some(&json!({"surface":"task_node"})),
        derivation_kind: Some("extract"),
        opened_at_epoch_ms: None,
        closed_at_epoch_ms: None,
        archived_at_epoch_ms: None,
        status_payload: &json!({}),
        metadata: &json!({}),
    };
    let filter = run_task_node_policy_scope_filter(&project, &namespace, &record);
    assert!(filter.owner_agent_required);
    assert!(!filter.owner_agent_present);
    assert!(filter.private_contour_violation);
    let error = validate_task_node_policy_scope_filter(&filter).expect_err("must fail closed");
    assert!(error.to_string().contains("requires owner_agent binding"));
}

#[test]
fn task_node_policy_scope_filter_rejects_quarantine_visibility() {
    let filter = super::TaskNodePolicyScopeFilter {
        visibility_scope: "quarantine".to_string(),
        project_code: "project_quarantine".to_string(),
        namespace_code: "review".to_string(),
        owner_agent_required: false,
        owner_agent_present: false,
        private_contour_violation: false,
        scope_allowed: false,
    };
    let error = validate_task_node_policy_scope_filter(&filter).expect_err("must fail closed");
    assert!(error.to_string().contains("dedicated quarantine_item path"));
}

#[test]
fn task_node_verification_conflict_check_detects_poisoned_payload() {
    let record = TaskNodeInsert {
        parent_task_node_id: None,
        memory_item_id: None,
        task_key: Some("task-poisoned"),
        task_role: Some("workline"),
        headline: "Poisoned task",
        summary: Some("summary"),
        next_step: Some("next"),
        execution_state: Some("proposed"),
        lifecycle_state: Some("hot"),
        confidence: None,
        current_score: None,
        reopened_count: None,
        child_count: None,
        closed_child_count: None,
        pending_return_count: None,
        source_event_ids: Some(&json!(["event:task-poisoned"])),
        artifact_refs: Some(&json!(["artifact://proof/task-poisoned"])),
        evidence_span: Some(&json!({"surface":"task_node"})),
        derivation_kind: Some("extract"),
        opened_at_epoch_ms: None,
        closed_at_epoch_ms: None,
        archived_at_epoch_ms: None,
        status_payload: &json!({"safety":{"poisoned":true}}),
        metadata: &json!({}),
    };
    assert!(task_node_marks_poisoned(&record));
    let check = TaskNodeVerificationConflictCheck {
        evidence_present: true,
        duplicate_task_key_conflict: false,
        poisoned_detected: true,
        private_contour_violation: false,
        task_key: Some("task-poisoned".to_string()),
        write_allowed: false,
    };
    let error = validate_task_node_verification_conflict_check(&check).expect_err("must fail");
    assert!(error.to_string().contains("flagged poisoned"));
}

#[test]
fn canonical_candidate_class_covers_all_classes() {
    assert_eq!(
        super::canonical_candidate_class_from_hints(None, Some("fact"), None, &[], false, "fact"),
        "fact"
    );
    assert_eq!(
        super::canonical_candidate_class_from_hints(
            None,
            Some("decision"),
            None,
            &[],
            false,
            "fact"
        ),
        "decision"
    );
    assert_eq!(
        super::canonical_candidate_class_from_hints(None, Some("task"), None, &[], false, "fact"),
        "commitment"
    );
    assert_eq!(
        super::canonical_candidate_class_from_hints(None, Some("skill"), None, &[], false, "fact"),
        "skill_hint"
    );
    assert_eq!(
        super::canonical_candidate_class_from_hints(
            None,
            Some("artifact"),
            None,
            &[],
            false,
            "fact"
        ),
        "artifact_ref"
    );
    assert_eq!(
        super::canonical_candidate_class_from_hints(
            Some("anti_pattern"),
            None,
            None,
            &[],
            false,
            "fact"
        ),
        "anti_pattern"
    );
    assert_eq!(
        super::canonical_candidate_class_from_hints(
            None,
            Some("failure_pattern"),
            None,
            &[],
            false,
            "fact"
        ),
        "failure_pattern"
    );
}

#[test]
fn skill_card_candidate_extraction_marks_runtime_contract() {
    let source_event_ids = vec!["event:skill".to_string()];
    let artifact_refs = vec!["artifact://proof/skill".to_string()];
    let evidence_span = json!({"path":"docs/skill.md","line_start":1,"line_end":2});
    let candidate = extract_skill_card_candidate(
        &source_event_ids,
        &artifact_refs,
        &evidence_span,
        None,
        Some("Stage3A skill"),
        Some("extract"),
    );
    assert_eq!(candidate.source_basis_status, "recorded");
    assert_eq!(candidate.source_event_count, 1);
    assert_eq!(candidate.artifact_ref_count, 1);
    assert!(candidate.has_evidence_span);
    assert_eq!(candidate.derivation_kind, "extract");
    assert_eq!(candidate.source_kind.as_deref(), Some("raw_event_append"));
    assert!(!candidate.hot_path_write_eligible);
    assert!(candidate.background_consolidation_recommended);
}

#[test]
fn skill_card_candidate_validation_rejects_basis_free_extract() {
    let candidate =
        extract_skill_card_candidate(&[], &[], &json!({}), None, Some("Skill"), Some("extract"));
    let error = validate_skill_card_candidate(&candidate).expect_err("basis-free skill rejected");
    assert!(error
            .to_string()
            .contains("skill card candidate requires recorded provenance basis unless derivation_kind=operator_write"));
}

#[test]
fn skill_card_policy_scope_filter_requires_agent_owner_for_agent_private() {
    let project = ProjectRecord {
        project_id: Uuid::nil(),
        code: "project_private".to_string(),
        display_name: "project_private".to_string(),
        repo_root: "/tmp/project_private".to_string(),
        visibility_scope: "agent_private".to_string(),
        updated_at: "2026-01-01T00:00:00Z".to_string(),
    };
    let namespace = NamespaceRecord {
        namespace_id: Uuid::nil(),
        code: "review".to_string(),
        display_name: "review".to_string(),
        retrieval_mode: "hybrid".to_string(),
    };
    let filter = run_skill_card_policy_scope_filter(&project, &namespace, "project");
    assert!(filter.owner_agent_required);
    assert!(!filter.owner_agent_present);
    assert!(filter.private_contour_violation);
    let error = validate_skill_card_policy_scope_filter(&filter).expect_err("must fail closed");
    assert!(
        error
            .to_string()
            .contains("requires agent-bound skill_owner_scope")
    );
}

#[test]
fn skill_card_policy_scope_filter_rejects_quarantine_visibility() {
    let filter = super::SkillCardPolicyScopeFilter {
        visibility_scope: "quarantine".to_string(),
        skill_owner_scope: "project".to_string(),
        project_code: "project_quarantine".to_string(),
        namespace_code: "review".to_string(),
        owner_agent_required: false,
        owner_agent_present: false,
        private_contour_violation: false,
        scope_allowed: false,
    };
    let error = validate_skill_card_policy_scope_filter(&filter).expect_err("must fail");
    assert!(error.to_string().contains("dedicated quarantine_item path"));
}

#[test]
fn skill_card_verification_conflict_check_detects_poisoned_evidence_span() {
    let evidence_span = json!({
        "poisoned": true,
        "surface": "skill_card"
    });
    assert!(evidence_span_marks_skill_card_poisoned(&evidence_span));
    let check = SkillCardVerificationConflictCheck {
        evidence_present: true,
        duplicate_version_conflict: false,
        poisoned_detected: true,
        private_contour_violation: false,
        skill_id: "skill.poisoned".to_string(),
        skill_version: 1,
        write_allowed: false,
    };
    let error = validate_skill_card_verification_conflict_check(&check).expect_err("must fail");
    assert!(error.to_string().contains("flagged poisoned"));
}

#[test]
fn task_node_candidate_extraction_marks_runtime_contract() {
    let source_event_ids = json!(["event:task-node"]);
    let artifact_refs = json!(["artifact://proof/task-node"]);
    let evidence_span = json!({"event_id":"event:task-node","snapshot_id":"snapshot:task-node"});
    let status_payload = json!({
        "source_kind": "continuity_handoff",
        "source_event_id": "event:task-node"
    });
    let metadata = json!({"local_path":"/tmp/task-node.md"});
    let candidate = extract_task_node_candidate(
        &TaskNodeInsert {
            parent_task_node_id: None,
            memory_item_id: None,
            task_key: Some("task-node-proof"),
            task_role: Some("proposal"),
            headline: "Decision: reopen workline",
            summary: Some("summary"),
            next_step: Some("do the thing"),
            execution_state: Some("active"),
            lifecycle_state: Some("hot"),
            confidence: Some(1.0),
            current_score: None,
            reopened_count: Some(0),
            child_count: Some(0),
            closed_child_count: Some(0),
            pending_return_count: Some(0),
            source_event_ids: Some(&source_event_ids),
            artifact_refs: Some(&artifact_refs),
            evidence_span: Some(&evidence_span),
            derivation_kind: Some("extract"),
            status_payload: &status_payload,
            metadata: &metadata,
            opened_at_epoch_ms: Some(1000),
            closed_at_epoch_ms: None,
            archived_at_epoch_ms: None,
        },
        &source_event_ids,
        &artifact_refs,
        &evidence_span,
    );
    assert_eq!(candidate.source_basis_status, "recorded");
    assert_eq!(candidate.source_event_count, 1);
    assert_eq!(candidate.artifact_ref_count, 1);
    assert!(candidate.has_evidence_span);
    assert_eq!(candidate.derivation_kind, "extract");
    assert_eq!(candidate.candidate_class, "commitment");
    assert_eq!(candidate.source_kind.as_deref(), Some("continuity_handoff"));
    assert!(candidate.hot_path_write_eligible);
    assert!(!candidate.background_consolidation_recommended);
}

#[test]
fn task_node_candidate_validation_rejects_basis_free_extract() {
    let candidate = TaskNodeCandidateExtraction {
        source_basis_status: "missing".to_string(),
        source_event_count: 0,
        artifact_ref_count: 0,
        has_evidence_span: false,
        candidate_class: "commitment".to_string(),
        derivation_kind: "extract".to_string(),
        source_kind: None,
        hot_path_write_eligible: true,
        background_consolidation_recommended: false,
    };
    let error =
        validate_task_node_candidate(&candidate).expect_err("basis-free task node rejected");
    assert!(error
            .to_string()
            .contains("task node candidate requires recorded provenance basis unless derivation_kind=operator_write"));
}

#[test]
fn task_event_validation_rejects_basis_free_raw_capture() {
    let payload = json!({});
    let error = validate_task_event_basis(
        &TaskEventInsert {
            task_node_id: Uuid::new_v4(),
            source_snapshot_id: None,
            source_event_id: None,
            event_kind: "created",
            prior_execution_state: None,
            next_execution_state: Some("active"),
            prior_lifecycle_state: None,
            next_lifecycle_state: Some("hot"),
            source_kind: None,
            artifact_refs: None,
            message_refs: None,
            evidence_span: None,
            derivation_kind: Some("raw_capture"),
            schema_version: Some("task-event-envelope-v1"),
            event_payload: &payload,
            recorded_at_epoch_ms: Some(1000),
        },
        &json!([]),
        &json!([]),
        &json!({}),
    )
    .expect_err("basis-free task event rejected");
    assert!(
        error
            .to_string()
            .contains("task event requires recorded basis unless derivation_kind=operator_write")
    );
}

#[test]
fn memory_link_decision_validation_rejects_basis_free_extract() {
    let payload = json!({});
    let error = validate_memory_link_decision_basis(
        &MemoryLinkDecisionInsert {
            task_node_id: None,
            retrieval_trace_id: None,
            candidate_task_node_id: None,
            decision_outcome: "abstain",
            legality_passed: false,
            scope_filter_passed: false,
            evidence_sufficient: false,
            classifier_label: None,
            classifier_score: None,
            decision_reason: Some("not enough evidence"),
            decision_payload: &payload,
            source_event_ids: None,
            artifact_refs: None,
            message_refs: None,
            evidence_span: None,
            derivation_kind: Some("extract"),
            schema_version: Some("memory-link-decision-envelope-v1"),
            recorded_at_epoch_ms: Some(1000),
        },
        &json!([]),
        &json!([]),
        &json!([]),
        &json!({}),
    )
    .expect_err("basis-free link decision rejected");
    assert!(error.to_string().contains(
            "memory link decision requires retrieval trace or recorded basis unless derivation_kind=operator_write"
        ));
}

#[test]
fn memory_link_decision_validation_rejects_escalate_without_additional_request() {
    let payload = json!({});
    let error = validate_memory_link_decision_basis(
        &MemoryLinkDecisionInsert {
            task_node_id: Some(Uuid::new_v4()),
            retrieval_trace_id: None,
            candidate_task_node_id: None,
            decision_outcome: "escalate",
            legality_passed: true,
            scope_filter_passed: true,
            evidence_sufficient: false,
            classifier_label: None,
            classifier_score: None,
            decision_reason: Some("need more proof"),
            decision_payload: &payload,
            source_event_ids: None,
            artifact_refs: None,
            message_refs: None,
            evidence_span: None,
            derivation_kind: Some("operator_write"),
            schema_version: Some("memory-link-decision-envelope-v1"),
            recorded_at_epoch_ms: Some(1000),
        },
        &json!([]),
        &json!([]),
        &json!([]),
        &json!({}),
    )
    .expect_err("escalate without request rejected");
    assert!(error.to_string().contains(
            "memory link decision outcome escalate requires decision_reason and decision_payload.additional_evidence_request"
        ));
}

#[test]
fn memory_link_decision_validation_rejects_pending_link_proposal_without_ttl_and_request() {
    let payload = json!({});
    let error = validate_memory_link_decision_basis(
        &MemoryLinkDecisionInsert {
            task_node_id: Some(Uuid::new_v4()),
            retrieval_trace_id: None,
            candidate_task_node_id: None,
            decision_outcome: "pending_link_proposal",
            legality_passed: true,
            scope_filter_passed: true,
            evidence_sufficient: false,
            classifier_label: None,
            classifier_score: None,
            decision_reason: Some("defer"),
            decision_payload: &payload,
            source_event_ids: None,
            artifact_refs: None,
            message_refs: None,
            evidence_span: None,
            derivation_kind: Some("operator_write"),
            schema_version: Some("memory-link-decision-envelope-v1"),
            recorded_at_epoch_ms: Some(1000),
        },
        &json!([]),
        &json!([]),
        &json!([]),
        &json!({}),
    )
    .expect_err("pending_link_proposal without ttl/request rejected");
    assert!(error.to_string().contains(
            "memory link decision outcome pending_link_proposal requires decision_reason, decision_payload.pending_link_ttl_epoch_ms and decision_payload.additional_evidence_request"
        ));
}

#[test]
fn memory_link_decision_validation_rejects_pending_link_proposal_without_reason() {
    let payload = json!({
        "pending_link_ttl_epoch_ms": 7777,
        "additional_evidence_request": "attach more evidence"
    });
    let error = validate_memory_link_decision_basis(
        &MemoryLinkDecisionInsert {
            task_node_id: Some(Uuid::new_v4()),
            retrieval_trace_id: None,
            candidate_task_node_id: None,
            decision_outcome: "pending_link_proposal",
            legality_passed: true,
            scope_filter_passed: true,
            evidence_sufficient: false,
            classifier_label: None,
            classifier_score: None,
            decision_reason: None,
            decision_payload: &payload,
            source_event_ids: None,
            artifact_refs: None,
            message_refs: None,
            evidence_span: None,
            derivation_kind: Some("operator_write"),
            schema_version: Some("memory-link-decision-envelope-v1"),
            recorded_at_epoch_ms: Some(1000),
        },
        &json!([]),
        &json!([]),
        &json!([]),
        &json!({}),
    )
    .expect_err("pending_link_proposal without reason rejected");
    assert!(error.to_string().contains(
            "memory link decision outcome pending_link_proposal requires decision_reason, decision_payload.pending_link_ttl_epoch_ms and decision_payload.additional_evidence_request"
        ));
}

#[test]
fn pending_link_proposal_validation_rejects_missing_ttl_and_evidence_request() {
    let payload = json!({});
    let error = validate_pending_link_proposal_basis(
        &PendingLinkProposalInsert {
            task_node_id: None,
            retrieval_trace_id: Some(Uuid::new_v4()),
            candidate_task_node_id: None,
            proposal_state: Some("pending"),
            proposal_reason: "needs more evidence",
            evidence_request: None,
            evidence_payload: &payload,
            classifier_score: Some(0.42),
            ttl_epoch_ms: None,
            source_event_ids: None,
            artifact_refs: None,
            message_refs: None,
            evidence_span: None,
            derivation_kind: Some("extract"),
            schema_version: Some("pending-link-proposal-envelope-v1"),
        },
        &json!([]),
        &json!([]),
        &json!([]),
        &json!({}),
    )
    .expect_err("pending proposal without ttl rejected");
    assert!(
        error
            .to_string()
            .contains("pending link proposal requires ttl_epoch_ms while proposal_state=pending")
    );
}

#[test]
fn artifact_ref_validation_rejects_basis_free_extract() {
    let metadata = json!({});
    let error = validate_artifact_ref_basis(
        &ArtifactRefInsert {
            project_id: Uuid::new_v4(),
            namespace_id: Uuid::new_v4(),
            artifact_kind: "context_pack",
            bucket: "proof-bucket",
            object_key: "proof/object.json",
            content_type: Some("application/json"),
            source_kind: None,
            source_event_ids: None,
            message_refs: None,
            evidence_span: None,
            derivation_kind: Some("extract"),
            schema_version: Some("artifact-ref-envelope-v1"),
            metadata: &metadata,
        },
        &json!([]),
        &json!([]),
        &json!({}),
    )
    .expect_err("basis-free artifact ref rejected");
    assert!(
        error
            .to_string()
            .contains("artifact ref requires recorded basis unless derivation_kind=operator_write")
    );
}

#[test]
fn skill_evidence_bundle_validation_rejects_basis_free_extract() {
    let error = validate_skill_evidence_bundle_basis(
        "extract",
        &json!([]),
        &json!([]),
        &json!([]),
        &json!({}),
    )
    .expect_err("basis-free skill evidence bundle rejected");
    assert!(error.to_string().contains(
        "skill evidence bundle requires recorded basis unless derivation_kind=operator_write"
    ));
}

#[test]
fn memory_relation_edge_validation_rejects_basis_free_extract() {
    let error = validate_memory_relation_edge_basis(
        "extract",
        &json!({}),
        &json!([]),
        &json!([]),
        &json!([]),
        &json!({}),
    )
    .expect_err("basis-free relation edge rejected");
    assert!(error
            .to_string()
            .contains("memory relation edge requires evidence or recorded basis unless derivation_kind=operator_write"));
}

#[test]
fn skill_activity_validation_rejects_basis_free_extract() {
    let error = validate_skill_activity_basis(
        "skill trigger match",
        "extract",
        &json!([]),
        &json!([]),
        &json!([]),
        &json!({}),
    )
    .expect_err("basis-free skill activity rejected");
    assert!(error.to_string().contains(
        "skill trigger match requires recorded basis unless derivation_kind=operator_write"
    ));
}

#[test]
fn memory_candidate_validation_rejects_basis_free_extract() {
    let empty: Vec<String> = Vec::new();
    let empty_span = json!({});
    let metadata = json!({});
    let record = MemoryItemInsert {
        source_project_code: None,
        import_packet_id: None,
        owner_agent_code: None,
        item_kind: "fact",
        identity_key: Some("stage2-proof"),
        title: "stage2 proof item",
        summary: Some("summary"),
        body: Some("body"),
        sensitivity_class: Some("confidential"),
        truth_state: Some("current"),
        trust_state: Some("verified"),
        verification_state: Some("verified"),
        lifecycle_state: Some("hot"),
        source_event_ids: &empty,
        artifact_refs: &empty,
        message_refs: &empty,
        evidence_span: &empty_span,
        derivation_kind: Some("extract"),
        observed_at_epoch_ms: Some(1000),
        recorded_at_epoch_ms: Some(1005),
        valid_from_epoch_ms: Some(1000),
        valid_to_epoch_ms: Some(2000),
        last_verified_at_epoch_ms: Some(1500),
        object_version: Some(2),
        causation_id: Some("cause-stage2"),
        correlation_id: Some("corr-stage2"),
        utility_score: Some(0.9),
        freshness_score: Some(0.8),
        retention_class: Some("durable"),
        ttl_epoch_ms: Some(60000),
        decay_policy: None,
        consolidation_status: None,
        imported_from: None,
        schema_version: Some("memory-envelope-v1"),
        superseded_by_memory_item_id: None,
        metadata: &metadata,
    };
    let candidate = super::MemoryItemCandidateExtraction {
        source_basis_status: "operator_only".to_string(),
        source_event_count: 0,
        artifact_ref_count: 0,
        message_ref_count: 0,
        has_evidence_span: false,
        source_kind: None,
        imported_from: json!({}),
        raw_event_kind: "memory_candidate_write".to_string(),
        raw_event_payload: json!({}),
        candidate_class: "fact".to_string(),
        hot_path_write_eligible: false,
        background_consolidation_recommended: true,
    };
    let error = validate_memory_item_candidate(&record, &candidate)
        .expect_err("basis-free extract must fail");
    assert!(
        error
            .to_string()
            .contains("recorded basis unless derivation_kind=operator_write")
    );
}

#[test]
fn stage2_runtime_metadata_is_augmented_for_read_projection() {
    let candidate = super::MemoryItemCandidateExtraction {
        source_basis_status: "recorded".to_string(),
        source_event_count: 1,
        artifact_ref_count: 1,
        message_ref_count: 1,
        has_evidence_span: true,
        source_kind: Some("proof_contract".to_string()),
        imported_from: json!({"source":"proof"}),
        raw_event_kind: "memory_candidate_write".to_string(),
        raw_event_payload: json!({}),
        candidate_class: "fact".to_string(),
        hot_path_write_eligible: false,
        background_consolidation_recommended: true,
    };
    let metadata = json!({"proof":"stage2"});
    let augmented = augment_memory_item_metadata_with_stage2_runtime(&metadata, &candidate);
    assert_eq!(augmented["proof"], json!("stage2"));
    assert_eq!(
        augmented["stage2_runtime"]["candidate_class"].as_str(),
        Some("fact")
    );
    assert_eq!(
        augmented["stage2_runtime"]["source_kind"].as_str(),
        Some("proof_contract")
    );
    assert_eq!(
        augmented["stage2_runtime"]["hot_path_write_eligible"].as_bool(),
        Some(false)
    );
    assert_eq!(
        augmented["stage2_runtime"]["background_consolidation_recommended"].as_bool(),
        Some(true)
    );
}

#[tokio::test]
async fn create_memory_item_materializes_raw_event_and_outbox() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let Some((key, value)) = trimmed.split_once('=') else {
                continue;
            };
            if std::env::var_os(key).is_none() {
                unsafe {
                    std::env::set_var(key, value);
                }
            }
        }
    }
    unsafe {
        std::env::set_var("AMI_STACK_NAME", "default");
    }
    let cfg = AppConfig::from_env().expect("env config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("unix epoch")
        .as_nanos();
    let event_id = format!("event:stage2:{suffix}");
    let identity_key = format!("stage2-raw-{suffix}");
    let artifact_ref = format!("artifact://proof/stage2/{suffix}");
    let source_event_ids = vec![event_id.clone()];
    let artifact_refs = vec![artifact_ref.clone()];
    let message_refs = vec![format!("message:{suffix}")];
    let evidence_span =
        json!({"path":"fixtures/project_alpha/src/lib.rs","line_start":1,"line_end":3});
    let metadata = json!({"proof":"stage2-raw-outbox"});
    let imported_from = json!({"source":"proof","kind":"local"});

    let memory_item = create_memory_item(
        &client,
        "project_alpha",
        "review",
        &MemoryItemInsert {
            source_project_code: None,
            import_packet_id: None,
            owner_agent_code: None,
            item_kind: "fact",
            identity_key: Some(identity_key.as_str()),
            title: "stage2 raw event proof item",
            summary: Some("summary"),
            body: Some("body"),
            sensitivity_class: Some("internal"),
            truth_state: Some("current"),
            trust_state: Some("verified"),
            verification_state: Some("verified"),
            lifecycle_state: Some("hot"),
            source_event_ids: &source_event_ids,
            artifact_refs: &artifact_refs,
            message_refs: &message_refs,
            evidence_span: &evidence_span,
            derivation_kind: Some("extract"),
            observed_at_epoch_ms: Some(1_000),
            recorded_at_epoch_ms: Some(1_005),
            valid_from_epoch_ms: Some(1_000),
            valid_to_epoch_ms: Some(2_000),
            last_verified_at_epoch_ms: Some(1_500),
            object_version: Some(1),
            causation_id: Some("cause-stage2-raw-outbox"),
            correlation_id: Some("corr-stage2-raw-outbox"),
            utility_score: Some(0.8),
            freshness_score: Some(0.7),
            retention_class: Some("durable"),
            ttl_epoch_ms: Some(60_000),
            decay_policy: None,
            consolidation_status: None,
            imported_from: Some(&imported_from),
            schema_version: Some("memory-envelope-v1"),
            superseded_by_memory_item_id: None,
            metadata: &metadata,
        },
    )
    .await
    .expect("memory item");

    let raw_provenance = client
        .query_one(
            r#"
                SELECT source_event_id
                FROM ami.memory_provenance
                WHERE memory_item_id = $1
                  AND source_kind = 'memory_raw_event_append'
                ORDER BY created_at DESC
                LIMIT 1
                "#,
            &[&memory_item.memory_item_id],
        )
        .await
        .expect("raw provenance");
    let raw_event_id =
        Uuid::parse_str(&raw_provenance.get::<_, String>(0)).expect("raw event uuid");

    let raw_event = client
        .query_one(
            r#"
                SELECT event_kind, source_event_ids, artifact_refs, message_refs, payload
                FROM ami.memory_raw_events
                WHERE memory_raw_event_id = $1
                "#,
            &[&raw_event_id],
        )
        .await
        .expect("raw event");
    assert_eq!(
        raw_event.get::<_, String>(0),
        "memory_candidate_write".to_string()
    );
    assert_eq!(raw_event.get::<_, serde_json::Value>(1), json!([event_id]));
    assert_eq!(
        raw_event.get::<_, serde_json::Value>(2),
        json!([artifact_ref])
    );
    assert_eq!(
        raw_event.get::<_, serde_json::Value>(3),
        json!([format!("message:{suffix}")])
    );
    assert_eq!(
        raw_event.get::<_, serde_json::Value>(4)["candidate"]["item_kind"].as_str(),
        Some("fact")
    );

    let outbox_rows = client
        .query(
            r#"
                SELECT subject, delivery_kind, delivery_state
                FROM ami.memory_write_outbox
                WHERE memory_item_id = $1
                ORDER BY subject
                "#,
            &[&memory_item.memory_item_id],
        )
        .await
        .expect("outbox rows");
    let subjects = outbox_rows
        .iter()
        .map(|row| row.get::<_, String>(0))
        .collect::<Vec<_>>();
    assert_eq!(outbox_rows.len(), 6);
    assert!(subjects.contains(&"ami.index.memory.lexical".to_string()));
    assert!(subjects.contains(&"ami.index.memory.graph".to_string()));
    assert!(subjects.contains(&"ami.index.memory.embedding".to_string()));
    assert!(subjects.contains(&"ami.index.memory.restore_summary".to_string()));
    assert!(subjects.contains(&"ami.event.memory_item.created".to_string()));
    assert!(subjects.contains(&"ami.event.memory_item.invalidate_cache".to_string()));
    assert!(
        outbox_rows
            .iter()
            .all(|row| row.get::<_, String>(2) == "pending")
    );
}

#[tokio::test]
async fn create_memory_item_rejects_duplicate_current_truth_identity_key() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let Some((key, value)) = trimmed.split_once('=') else {
                continue;
            };
            if std::env::var_os(key).is_none() {
                unsafe {
                    std::env::set_var(key, value);
                }
            }
        }
    }
    unsafe {
        std::env::set_var("AMI_STACK_NAME", "default");
    }
    let cfg = AppConfig::from_env().expect("env config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("unix epoch")
        .as_nanos();
    let identity_key = format!("stage2-duplicate-current-{suffix}");
    let first_source_event_ids = vec![format!("event:first:{suffix}")];
    let second_source_event_ids = vec![format!("event:second:{suffix}")];
    let empty_artifacts: Vec<String> = Vec::new();
    let empty_messages: Vec<String> = Vec::new();
    let evidence_span = json!({"source":"proof","kind":"raw_log","range":"1-2"});
    let metadata = json!({"proof":"duplicate-current-truth"});
    let imported_from = json!({"source":"proof","kind":"local"});

    create_memory_item(
        &client,
        "project_alpha",
        "review",
        &MemoryItemInsert {
            source_project_code: None,
            import_packet_id: None,
            owner_agent_code: None,
            item_kind: "fact",
            identity_key: Some(&identity_key),
            title: "duplicate current truth first",
            summary: Some("summary"),
            body: None,
            sensitivity_class: Some("internal"),
            truth_state: Some("current"),
            trust_state: Some("verified"),
            verification_state: Some("verified"),
            lifecycle_state: Some("hot"),
            source_event_ids: &first_source_event_ids,
            artifact_refs: &empty_artifacts,
            message_refs: &empty_messages,
            evidence_span: &evidence_span,
            derivation_kind: Some("extract"),
            observed_at_epoch_ms: Some(1),
            recorded_at_epoch_ms: Some(1),
            valid_from_epoch_ms: Some(1),
            valid_to_epoch_ms: None,
            last_verified_at_epoch_ms: Some(1),
            object_version: Some(1),
            causation_id: None,
            correlation_id: None,
            utility_score: None,
            freshness_score: None,
            retention_class: Some("standard"),
            ttl_epoch_ms: None,
            decay_policy: None,
            consolidation_status: None,
            imported_from: Some(&imported_from),
            schema_version: Some("memory-envelope-v1"),
            superseded_by_memory_item_id: None,
            metadata: &metadata,
        },
    )
    .await
    .expect("first current identity write");

    let error = create_memory_item(
        &client,
        "project_alpha",
        "review",
        &MemoryItemInsert {
            source_project_code: None,
            import_packet_id: None,
            owner_agent_code: None,
            item_kind: "fact",
            identity_key: Some(&identity_key),
            title: "duplicate current truth second",
            summary: Some("summary"),
            body: None,
            sensitivity_class: Some("internal"),
            truth_state: Some("current"),
            trust_state: Some("verified"),
            verification_state: Some("verified"),
            lifecycle_state: Some("hot"),
            source_event_ids: &second_source_event_ids,
            artifact_refs: &empty_artifacts,
            message_refs: &empty_messages,
            evidence_span: &evidence_span,
            derivation_kind: Some("extract"),
            observed_at_epoch_ms: Some(2),
            recorded_at_epoch_ms: Some(2),
            valid_from_epoch_ms: Some(2),
            valid_to_epoch_ms: None,
            last_verified_at_epoch_ms: Some(2),
            object_version: Some(1),
            causation_id: None,
            correlation_id: None,
            utility_score: None,
            freshness_score: None,
            retention_class: Some("standard"),
            ttl_epoch_ms: None,
            decay_policy: None,
            consolidation_status: None,
            imported_from: Some(&imported_from),
            schema_version: Some("memory-envelope-v1"),
            superseded_by_memory_item_id: None,
            metadata: &metadata,
        },
    )
    .await
    .expect_err("duplicate current identity must fail");
    assert!(
        error
            .to_string()
            .contains("conflicts with existing current truth")
    );
}

#[tokio::test]
async fn create_memory_item_requires_controlled_import_packet_for_cross_project_basis() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let Some((key, value)) = trimmed.split_once('=') else {
                continue;
            };
            if std::env::var_os(key).is_none() {
                unsafe {
                    std::env::set_var(key, value);
                }
            }
        }
    }
    unsafe {
        std::env::set_var("AMI_STACK_NAME", "default");
    }
    let cfg = AppConfig::from_env().expect("env config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("unix epoch")
        .as_nanos();
    let (_workspace_code, source_project_code, target_project_code, _transfer_policy_code) =
        create_stage2_import_shared_context(&client, suffix).await;
    let target_project = get_project_by_code(&client, &target_project_code)
        .await
        .expect("target project");
    ensure_namespace(
        &client,
        target_project.project_id,
        "review",
        Some("review"),
        "local_strict",
    )
    .await
    .expect("target namespace");
    let source_event_ids = vec![format!("event:controlled-import:{suffix}")];
    let artifact_refs = vec![format!("artifact://proof/controlled-import/{suffix}")];
    let message_refs = vec![format!("message:controlled-import:{suffix}")];
    let evidence_span = json!({"kind":"memory_item","case":"missing-import-packet"});
    let metadata = json!({"proof":"stage6-controlled-transfer"});

    let error = create_memory_item(
        &client,
        &target_project_code,
        "review",
        &MemoryItemInsert {
            source_project_code: Some(&source_project_code),
            import_packet_id: None,
            owner_agent_code: None,
            item_kind: "fact",
            identity_key: Some(&format!("stage6-controlled-{suffix}")),
            title: "cross project basis without import packet",
            summary: Some("summary"),
            body: None,
            sensitivity_class: Some("internal"),
            truth_state: Some("current"),
            trust_state: Some("verified"),
            verification_state: Some("verified"),
            lifecycle_state: Some("hot"),
            source_event_ids: &source_event_ids,
            artifact_refs: &artifact_refs,
            message_refs: &message_refs,
            evidence_span: &evidence_span,
            derivation_kind: Some("extract"),
            observed_at_epoch_ms: Some(1_000),
            recorded_at_epoch_ms: Some(1_000),
            valid_from_epoch_ms: Some(1_000),
            valid_to_epoch_ms: None,
            last_verified_at_epoch_ms: Some(1_000),
            object_version: Some(1),
            causation_id: None,
            correlation_id: None,
            utility_score: None,
            freshness_score: None,
            retention_class: Some("standard"),
            ttl_epoch_ms: None,
            decay_policy: None,
            consolidation_status: None,
            imported_from: None,
            schema_version: Some("memory-envelope-v1"),
            superseded_by_memory_item_id: None,
            metadata: &metadata,
        },
    )
    .await
    .expect_err("cross-project basis without controlled import packet must fail");
    assert!(
        error
            .to_string()
            .contains("requires controlled import_packet")
    );
}

#[tokio::test]
async fn create_memory_item_rejects_import_packet_target_mismatch() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let Some((key, value)) = trimmed.split_once('=') else {
                continue;
            };
            if std::env::var_os(key).is_none() {
                unsafe {
                    std::env::set_var(key, value);
                }
            }
        }
    }
    unsafe {
        std::env::set_var("AMI_STACK_NAME", "default");
    }
    let cfg = AppConfig::from_env().expect("env config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("unix epoch")
        .as_nanos();
    let (_workspace_code, source_project_code, target_project_code, transfer_policy_code) =
        create_stage2_import_shared_context(&client, suffix).await;
    let (_other_workspace, _other_source, other_target_project_code, _other_transfer_policy) =
        create_stage2_import_shared_context(&client, suffix + 1).await;
    let target_project = get_project_by_code(&client, &target_project_code)
        .await
        .expect("target project");
    ensure_namespace(
        &client,
        target_project.project_id,
        "review",
        Some("review"),
        "local_strict",
    )
    .await
    .expect("target namespace");
    let other_target_project = get_project_by_code(&client, &other_target_project_code)
        .await
        .expect("other target project");
    ensure_namespace(
        &client,
        other_target_project.project_id,
        "review",
        Some("review"),
        "local_strict",
    )
    .await
    .expect("other target namespace");
    let packet = create_import_packet(
        &client,
        &source_project_code,
        &target_project_code,
        Some(&transfer_policy_code),
        None,
        "borrowed_unverified",
        Some("import summary"),
        Some("controlled transfer"),
        "cross_project_linked",
        "proposed",
        "unverified",
        "borrowed",
        false,
        &[format!("memory-item:{suffix}")],
        &[format!("artifact://proof/import-packet/{suffix}")],
        Some("project_link_import"),
        Some(&json!([format!("event:import-packet:{suffix}")])),
        Some(&json!([format!("message:import-packet:{suffix}")])),
        Some(&json!({"kind":"import_packet","case":"target-match"})),
        Some("import"),
        Some("import-packet-envelope-v1"),
    )
    .await
    .expect("import packet");
    let source_event_ids = vec![format!("event:import-mismatch:{suffix}")];
    let artifact_refs = vec![format!("artifact://proof/import-mismatch/{suffix}")];
    let message_refs = vec![format!("message:import-mismatch:{suffix}")];
    let evidence_span = json!({"kind":"memory_item","case":"import-target-mismatch"});
    let metadata = json!({"proof":"stage6-import-target-mismatch"});

    let error = create_memory_item(
        &client,
        &other_target_project_code,
        "review",
        &MemoryItemInsert {
            source_project_code: Some(&source_project_code),
            import_packet_id: Some(packet.import_packet_id),
            owner_agent_code: None,
            item_kind: "fact",
            identity_key: Some(&format!("stage6-import-mismatch-{suffix}")),
            title: "cross project basis with target mismatch",
            summary: Some("summary"),
            body: None,
            sensitivity_class: Some("internal"),
            truth_state: Some("current"),
            trust_state: Some("verified"),
            verification_state: Some("verified"),
            lifecycle_state: Some("hot"),
            source_event_ids: &source_event_ids,
            artifact_refs: &artifact_refs,
            message_refs: &message_refs,
            evidence_span: &evidence_span,
            derivation_kind: Some("extract"),
            observed_at_epoch_ms: Some(2_000),
            recorded_at_epoch_ms: Some(2_000),
            valid_from_epoch_ms: Some(2_000),
            valid_to_epoch_ms: None,
            last_verified_at_epoch_ms: Some(2_000),
            object_version: Some(1),
            causation_id: None,
            correlation_id: None,
            utility_score: None,
            freshness_score: None,
            retention_class: Some("standard"),
            ttl_epoch_ms: None,
            decay_policy: None,
            consolidation_status: None,
            imported_from: None,
            schema_version: Some("memory-envelope-v1"),
            superseded_by_memory_item_id: None,
            metadata: &metadata,
        },
    )
    .await
    .expect_err("mismatched import packet target must fail");
    assert!(
        error
            .to_string()
            .contains("import_packet target project does not match target contour")
    );
}

#[tokio::test]
async fn create_memory_item_with_borrowed_import_packet_keeps_imported_visibility_scope() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let Some((key, value)) = trimmed.split_once('=') else {
                continue;
            };
            if std::env::var_os(key).is_none() {
                unsafe {
                    std::env::set_var(key, value);
                }
            }
        }
    }
    unsafe {
        std::env::set_var("AMI_STACK_NAME", "default");
    }
    let cfg = AppConfig::from_env().expect("env config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("unix epoch")
        .as_nanos();
    let (_workspace_code, source_project_code, target_project_code, transfer_policy_code) =
        create_stage2_import_shared_context(&client, suffix).await;
    let target_project = get_project_by_code(&client, &target_project_code)
        .await
        .expect("target project");
    ensure_namespace(
        &client,
        target_project.project_id,
        "review",
        Some("review"),
        "local_strict",
    )
    .await
    .expect("target namespace");
    let packet = create_import_packet(
        &client,
        &source_project_code,
        &target_project_code,
        Some(&transfer_policy_code),
        None,
        "borrowed_unverified",
        Some("import summary"),
        Some("controlled transfer"),
        "cross_project_linked",
        "proposed",
        "unverified",
        "borrowed",
        false,
        &[format!("memory-item:{suffix}")],
        &[format!("artifact://proof/import-visibility/{suffix}")],
        Some("project_link_import"),
        Some(&json!([format!("event:import-packet:{suffix}")])),
        Some(&json!([format!("message:import-packet:{suffix}")])),
        Some(&json!({"kind":"import_packet","case":"borrowed-imported-visibility"})),
        Some("import"),
        Some("import-packet-envelope-v1"),
    )
    .await
    .expect("import packet");
    let source_event_ids = vec![format!("event:import-visibility:{suffix}")];
    let artifact_refs = vec![format!("artifact://proof/import-visibility/{suffix}")];
    let message_refs = vec![format!("message:import-visibility:{suffix}")];
    let evidence_span = json!({"kind":"memory_item","case":"borrowed-imported-visibility"});
    let metadata = json!({"proof":"stage6-imported-visibility"});

    let item = create_memory_item(
        &client,
        &target_project_code,
        "review",
        &MemoryItemInsert {
            source_project_code: Some(&source_project_code),
            import_packet_id: Some(packet.import_packet_id),
            owner_agent_code: None,
            item_kind: "fact",
            identity_key: Some(&format!("stage6-imported-visibility-{suffix}")),
            title: "cross project borrowed item",
            summary: Some("summary"),
            body: None,
            sensitivity_class: Some("internal"),
            truth_state: Some("proposed"),
            trust_state: Some("proposed"),
            verification_state: Some("unverified"),
            lifecycle_state: Some("hot"),
            source_event_ids: &source_event_ids,
            artifact_refs: &artifact_refs,
            message_refs: &message_refs,
            evidence_span: &evidence_span,
            derivation_kind: Some("extract"),
            observed_at_epoch_ms: Some(2_000),
            recorded_at_epoch_ms: Some(2_000),
            valid_from_epoch_ms: Some(2_000),
            valid_to_epoch_ms: None,
            last_verified_at_epoch_ms: None,
            object_version: Some(1),
            causation_id: None,
            correlation_id: None,
            utility_score: None,
            freshness_score: None,
            retention_class: Some("standard"),
            ttl_epoch_ms: None,
            decay_policy: None,
            consolidation_status: None,
            imported_from: None,
            schema_version: Some("memory-envelope-v1"),
            superseded_by_memory_item_id: None,
            metadata: &metadata,
        },
    )
    .await
    .expect("borrowed import memory item");

    assert_eq!(item.visibility_scope, "imported");
    assert_eq!(item.import_packet_id, Some(packet.import_packet_id));
}

#[tokio::test]
async fn relay_memory_write_outbox_marks_rows_published() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let Some((key, value)) = trimmed.split_once('=') else {
                continue;
            };
            if std::env::var_os(key).is_none() {
                unsafe {
                    std::env::set_var(key, value);
                }
            }
        }
    }
    unsafe {
        std::env::set_var("AMI_STACK_NAME", "default");
    }
    let cfg = AppConfig::from_env().expect("env config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("unix epoch")
        .as_nanos();
    let source_event_ids = vec![format!("event:relay:{suffix}")];
    let artifact_refs = vec![format!("artifact://proof/relay/{suffix}")];
    let message_refs = vec![format!("message:relay:{suffix}")];
    let identity_key = format!("stage2-relay-{suffix}");
    let evidence_span =
        json!({"path":"fixtures/project_alpha/src/lib.rs","line_start":1,"line_end":2});
    let metadata = json!({"proof":"stage2-relay"});
    let imported_from = json!({"source":"proof","kind":"local"});

    let memory_item = create_memory_item(
        &client,
        "project_alpha",
        "review",
        &MemoryItemInsert {
            source_project_code: None,
            import_packet_id: None,
            owner_agent_code: None,
            item_kind: "fact",
            identity_key: Some(&identity_key),
            title: "stage2 relay proof item",
            summary: Some("summary"),
            body: Some("body"),
            sensitivity_class: Some("internal"),
            truth_state: Some("current"),
            trust_state: Some("verified"),
            verification_state: Some("verified"),
            lifecycle_state: Some("hot"),
            source_event_ids: &source_event_ids,
            artifact_refs: &artifact_refs,
            message_refs: &message_refs,
            evidence_span: &evidence_span,
            derivation_kind: Some("extract"),
            observed_at_epoch_ms: Some(10),
            recorded_at_epoch_ms: Some(11),
            valid_from_epoch_ms: Some(10),
            valid_to_epoch_ms: Some(20),
            last_verified_at_epoch_ms: Some(15),
            object_version: Some(1),
            causation_id: Some("cause-stage2-relay"),
            correlation_id: Some("corr-stage2-relay"),
            utility_score: Some(0.4),
            freshness_score: Some(0.4),
            retention_class: Some("durable"),
            ttl_epoch_ms: Some(10_000),
            decay_policy: None,
            consolidation_status: None,
            imported_from: Some(&imported_from),
            schema_version: Some("memory-envelope-v1"),
            superseded_by_memory_item_id: None,
            metadata: &metadata,
        },
    )
    .await
    .expect("memory item");

    let pending_rows = client
        .query(
            r#"
                SELECT subject, delivery_state
                FROM ami.memory_write_outbox
                WHERE memory_item_id = $1
                ORDER BY created_at ASC
                "#,
            &[&memory_item.memory_item_id],
        )
        .await
        .expect("pending outbox rows");
    assert!(pending_rows.len() >= 6);
    assert!(
        pending_rows
            .iter()
            .all(|row| row.get::<_, String>(1) == "pending")
    );

    let published = nats::relay_memory_write_outbox(&cfg, &client, 4096)
        .await
        .expect("relay outbox");
    assert!(published >= 6);

    let states = client
        .query(
            r#"
                SELECT delivery_state, published_at_epoch_ms
                FROM ami.memory_write_outbox
                WHERE memory_item_id = $1
                ORDER BY subject
                "#,
            &[&memory_item.memory_item_id],
        )
        .await
        .expect("load states");
    assert_eq!(states.len(), 6);
    assert!(
        states
            .iter()
            .all(|row| row.get::<_, String>(0) == "published")
    );
    assert!(
        states
            .iter()
            .all(|row| row.get::<_, Option<i64>>(1).is_some())
    );
}

#[tokio::test]
async fn memory_envelope_view_surfaces_stage2_runtime_fields() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let Some((key, value)) = trimmed.split_once('=') else {
                continue;
            };
            if std::env::var_os(key).is_none() {
                unsafe {
                    std::env::set_var(key, value);
                }
            }
        }
    }
    unsafe {
        std::env::set_var("AMI_STACK_NAME", "default");
    }
    let cfg = AppConfig::from_env().expect("env config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("unix epoch")
        .as_nanos();
    let source_event_ids = vec![format!("event:view:{suffix}")];
    let artifact_refs = vec![format!("artifact://proof/view/{suffix}")];
    let message_refs = vec![format!("message:view:{suffix}")];
    let identity_key = format!("stage2-view-{suffix}");
    let evidence_span =
        json!({"path":"fixtures/project_alpha/src/lib.rs","line_start":1,"line_end":2});
    let metadata = json!({"proof":"stage2-view"});
    let imported_from = json!({"source":"proof","kind":"local"});

    let memory_item = create_memory_item(
        &client,
        "project_alpha",
        "review",
        &MemoryItemInsert {
            source_project_code: None,
            import_packet_id: None,
            owner_agent_code: None,
            item_kind: "fact",
            identity_key: Some(&identity_key),
            title: "stage2 envelope view item",
            summary: Some("summary"),
            body: Some("body"),
            sensitivity_class: Some("internal"),
            truth_state: Some("current"),
            trust_state: Some("verified"),
            verification_state: Some("verified"),
            lifecycle_state: Some("hot"),
            source_event_ids: &source_event_ids,
            artifact_refs: &artifact_refs,
            message_refs: &message_refs,
            evidence_span: &evidence_span,
            derivation_kind: Some("extract"),
            observed_at_epoch_ms: Some(10),
            recorded_at_epoch_ms: Some(11),
            valid_from_epoch_ms: Some(10),
            valid_to_epoch_ms: Some(20),
            last_verified_at_epoch_ms: Some(15),
            object_version: Some(1),
            causation_id: Some("cause-stage2-view"),
            correlation_id: Some("corr-stage2-view"),
            utility_score: Some(0.4),
            freshness_score: Some(0.4),
            retention_class: Some("durable"),
            ttl_epoch_ms: Some(10_000),
            decay_policy: None,
            consolidation_status: None,
            imported_from: Some(&imported_from),
            schema_version: Some("memory-envelope-v1"),
            superseded_by_memory_item_id: None,
            metadata: &metadata,
        },
    )
    .await
    .expect("memory item");

    let envelope = client
            .query_one(
                r#"
                SELECT candidate_class, source_kind, hot_path_write_eligible, background_consolidation_recommended
                FROM ami.memory_envelopes
                WHERE memory_id = $1
                "#,
                &[&memory_item.memory_item_id],
            )
            .await
            .expect("memory envelope");
    assert_eq!(envelope.get::<_, String>(0), "fact".to_string());
    assert_eq!(
        envelope.get::<_, Option<String>>(1),
        Some("raw_event_append".to_string())
    );
    assert!(!envelope.get::<_, bool>(2));
    assert!(envelope.get::<_, bool>(3));
}

#[tokio::test]
async fn create_memory_card_surfaces_stage2_runtime_fields() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let Some((key, value)) = trimmed.split_once('=') else {
                continue;
            };
            if std::env::var_os(key).is_none() {
                unsafe {
                    std::env::set_var(key, value);
                }
            }
        }
    }
    unsafe {
        std::env::set_var("AMI_STACK_NAME", "default");
    }
    let cfg = AppConfig::from_env().expect("env config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("unix epoch")
        .as_nanos();
    let tags = vec!["decision".to_string()];
    let provenance = json!({
        "source_event_ids": [format!("event:card:{suffix}")],
        "artifact_refs": [format!("artifact://proof/card/{suffix}")],
        "message_refs": [format!("message:card:{suffix}")],
        "evidence_span": {"path":"docs/card.md","line_start":1,"line_end":2}
    });

    let card = create_memory_card(
        &client,
        "project_alpha",
        "review",
        "stage2 card",
        "summary",
        "body",
        &tags,
        &provenance,
        Some(&format!("subject:{suffix}")),
        Some(&format!("predicate:{suffix}")),
        Some(&format!("object:{suffix}")),
        Some("current"),
        Some("verified"),
        Some("active"),
        Some(10),
        Some(11),
        Some(10),
        Some(20),
        Some(15),
    )
    .await
    .expect("memory card");

    assert_eq!(card.derivation_kind, "extract");
    assert_eq!(card.candidate_class, "decision");
    assert_eq!(card.source_kind.as_deref(), Some("raw_event_append"));
    assert!(card.hot_path_write_eligible);
    assert!(!card.background_consolidation_recommended);
}

#[tokio::test]
async fn create_memory_card_rejects_duplicate_current_truth_fact_triple() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let Some((key, value)) = trimmed.split_once('=') else {
                continue;
            };
            if std::env::var_os(key).is_none() {
                unsafe {
                    std::env::set_var(key, value);
                }
            }
        }
    }
    unsafe {
        std::env::set_var("AMI_STACK_NAME", "default");
    }
    let cfg = AppConfig::from_env().expect("env config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("unix epoch")
        .as_nanos();
    let fact_subject = format!("subject:{suffix}");
    let fact_predicate = format!("predicate:{suffix}");
    let fact_object = format!("object:{suffix}");
    let first_provenance = json!({
        "source_event_ids": [format!("event:card:first:{suffix}")],
        "artifact_refs": [format!("artifact://proof/card/first/{suffix}")],
        "message_refs": [format!("message:card:first:{suffix}")],
        "evidence_span": {"path":"docs/card.md","line_start":1,"line_end":2}
    });
    create_memory_card(
        &client,
        "project_alpha",
        "review",
        "first current card",
        "summary",
        "body",
        &[],
        &first_provenance,
        Some(&fact_subject),
        Some(&fact_predicate),
        Some(&fact_object),
        Some("current"),
        Some("verified"),
        Some("active"),
        Some(10),
        Some(11),
        Some(10),
        Some(20),
        Some(15),
    )
    .await
    .expect("first memory card");

    let second_provenance = json!({
        "source_event_ids": [format!("event:card:second:{suffix}")],
        "artifact_refs": [format!("artifact://proof/card/second/{suffix}")],
        "message_refs": [format!("message:card:second:{suffix}")],
        "evidence_span": {"path":"docs/card.md","line_start":3,"line_end":4}
    });
    let error = create_memory_card(
        &client,
        "project_alpha",
        "review",
        "second current card",
        "summary",
        "body",
        &[],
        &second_provenance,
        Some(&fact_subject),
        Some(&fact_predicate),
        Some(&fact_object),
        Some("current"),
        Some("verified"),
        Some("active"),
        Some(12),
        Some(13),
        Some(12),
        Some(22),
        Some(16),
    )
    .await
    .expect_err("duplicate current truth card rejected");
    assert!(
        error
            .to_string()
            .contains("existing current active truth for the same fact triple")
    );
}

#[tokio::test]
async fn apply_memory_card_update_supersedes_prior_current_fact_for_same_subject_predicate_and_preserves_temporal_slices()
 {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let Some((key, value)) = trimmed.split_once('=') else {
                continue;
            };
            if std::env::var_os(key).is_none() {
                unsafe {
                    std::env::set_var(key, value);
                }
            }
        }
    }
    unsafe {
        std::env::set_var("AMI_STACK_NAME", "default");
    }
    let cfg = AppConfig::from_env().expect("env config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("unix epoch")
        .as_nanos();
    let fact_subject = format!("user:{suffix}");
    let fact_predicate = "lives_in".to_string();
    let old_fact_object = format!("city:paris:{suffix}");
    let new_fact_object = format!("city:london:{suffix}");
    let old_card = apply_memory_card_update(
        &client,
        "project_alpha",
        "review",
        &format!("Residence fact Paris {suffix}"),
        "User currently lives in Paris.",
        "historical residence proof paris",
        &["semantic".to_string(), "temporal".to_string()],
        &json!({
            "source_event_ids": [format!("event:semantic-old:{suffix}")],
            "artifact_refs": [format!("artifact://proof/semantic-old/{suffix}")],
            "message_refs": [format!("thread:semantic-old:{suffix}")],
            "evidence_span": {"kind":"memory_card","case":"semantic_temporal_old"},
            "source_kind": "semantic_temporal_seed"
        }),
        Some(&fact_subject),
        Some(&fact_predicate),
        Some(&old_fact_object),
        Some("current"),
        Some("verified"),
        Some("active"),
        Some(1_000),
        Some(1_001),
        Some(1_000),
        None,
        Some(1_002),
    )
    .await
    .expect("old card");

    let new_card = apply_memory_card_update(
        &client,
        "project_alpha",
        "review",
        &format!("Residence fact London {suffix}"),
        "User currently lives in London.",
        "historical residence proof london",
        &["semantic".to_string(), "temporal".to_string()],
        &json!({
            "source_event_ids": [format!("event:semantic-new:{suffix}")],
            "artifact_refs": [format!("artifact://proof/semantic-new/{suffix}")],
            "message_refs": [format!("thread:semantic-new:{suffix}")],
            "evidence_span": {"kind":"memory_card","case":"semantic_temporal_new"},
            "source_kind": "semantic_temporal_seed"
        }),
        Some(&fact_subject),
        Some(&fact_predicate),
        Some(&new_fact_object),
        Some("current"),
        Some("verified"),
        Some("active"),
        Some(2_000),
        Some(2_001),
        Some(2_000),
        None,
        Some(2_002),
    )
    .await
    .expect("new card");

    let old_row = client
        .query_one(
            r#"
                SELECT truth_state, status, superseded_by_memory_card_id, valid_to_epoch_ms
                FROM ami.memory_cards
                WHERE memory_card_id = $1
                "#,
            &[&old_card.memory_card_id],
        )
        .await
        .expect("old card refresh");
    assert_eq!(old_row.get::<_, String>(0), "superseded");
    assert_eq!(old_row.get::<_, String>(1), "superseded");
    assert_eq!(
        old_row.get::<_, Option<Uuid>>(2),
        Some(new_card.memory_card_id)
    );
    assert_eq!(old_row.get::<_, Option<i64>>(3), Some(2_000));

    let relation_row = client
        .query_one(
            r#"
                SELECT relation_type, evidence
                FROM ami.memory_relation_edges
                WHERE source_memory_card_id = $1
                  AND target_memory_card_id = $2
                  AND relation_type = 'supersedes'
                "#,
            &[&new_card.memory_card_id, &old_card.memory_card_id],
        )
        .await
        .expect("supersedes relation");
    assert_eq!(relation_row.get::<_, String>(0), "supersedes");
    let relation_evidence: Value = relation_row.get(1);
    assert_eq!(
        relation_evidence["supersession_reason"],
        json!("knowledge_update_object_change")
    );
    assert_eq!(relation_evidence["old_fact_object"], json!(old_fact_object));
    assert_eq!(relation_evidence["new_fact_object"], json!(new_fact_object));

    let transition_row = client
        .query_one(
            r#"
                SELECT to_truth_state, to_status, transition_reason, effective_at_epoch_ms
                FROM ami.memory_card_transitions
                WHERE memory_card_id = $1
                ORDER BY recorded_at_epoch_ms DESC
                LIMIT 1
                "#,
            &[&old_card.memory_card_id],
        )
        .await
        .expect("transition row");
    assert_eq!(
        transition_row.get::<_, Option<String>>(0).as_deref(),
        Some("superseded")
    );
    assert_eq!(
        transition_row.get::<_, Option<String>>(1).as_deref(),
        Some("superseded")
    );
    assert_eq!(
        transition_row.get::<_, Option<String>>(2).as_deref(),
        Some("superseded")
    );
    assert_eq!(transition_row.get::<_, Option<i64>>(3), Some(2_000));

    let project = get_project_by_code(&client, "project_alpha")
        .await
        .expect("project");
    let namespace = get_namespace_by_code(&client, project.project_id, "review")
        .await
        .expect("namespace");

    let historical_hits = search_memory_cards_for_namespace(
        &client,
        project.project_id,
        namespace.namespace_id,
        &fact_subject,
        10,
        Some(1_500),
    )
    .await
    .expect("historical hits");
    assert!(
        historical_hits
            .iter()
            .any(|card| card.memory_card_id == old_card.memory_card_id)
    );
    assert!(
        historical_hits
            .iter()
            .all(|card| card.memory_card_id != new_card.memory_card_id)
    );

    let latest_hits = search_memory_cards_for_namespace(
        &client,
        project.project_id,
        namespace.namespace_id,
        &fact_subject,
        10,
        None,
    )
    .await
    .expect("latest hits");
    assert!(
        latest_hits
            .iter()
            .any(|card| card.memory_card_id == new_card.memory_card_id)
    );
    assert!(
        latest_hits
            .iter()
            .all(|card| card.memory_card_id != old_card.memory_card_id)
    );

    let future_hits = search_memory_cards_for_namespace(
        &client,
        project.project_id,
        namespace.namespace_id,
        &fact_subject,
        10,
        Some(2_500),
    )
    .await
    .expect("future hits");
    assert!(
        future_hits
            .iter()
            .any(|card| card.memory_card_id == new_card.memory_card_id)
    );
    assert!(
        future_hits
            .iter()
            .all(|card| card.memory_card_id != old_card.memory_card_id)
    );

    let future_only_query = format!("When did {} move to us-east?", fact_subject);
    let pre_update_hits = search_memory_cards_for_namespace(
        &client,
        project.project_id,
        namespace.namespace_id,
        &future_only_query,
        10,
        Some(1_500),
    )
    .await
    .expect("pre-update future-only hits");
    assert!(pre_update_hits.is_empty());
}

#[tokio::test]
async fn search_memory_cards_matches_generic_nl_queries_against_fact_fields_and_time_slice() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let Some((key, value)) = trimmed.split_once('=') else {
                continue;
            };
            if std::env::var_os(key).is_none() {
                unsafe {
                    std::env::set_var(key, value);
                }
            }
        }
    }
    unsafe {
        std::env::set_var("AMI_STACK_NAME", "default");
    }
    let cfg = AppConfig::from_env().expect("env config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("unix epoch")
        .as_nanos();
    let fact_subject = format!("infra.server.region.{suffix}");
    let fact_predicate = "current_region".to_string();
    let old_fact_object = "eu-west".to_string();
    let new_fact_object = "us-east".to_string();

    let old_card = apply_memory_card_update(
        &client,
        "project_alpha",
        "review",
        &format!("Server region fact v1 {suffix}"),
        "Server region is eu-west.",
        "Manual-style semantic temporal check old region.",
        &["semantic".to_string(), "temporal".to_string()],
        &json!({
            "source_event_ids": [format!("event:server-region-old:{suffix}")],
            "artifact_refs": [format!("artifact://proof/server-region-old/{suffix}")],
            "message_refs": [format!("thread:server-region-old:{suffix}")],
            "evidence_span": {"kind":"memory_card","case":"server_region_old"},
            "source_kind": "semantic_temporal_seed"
        }),
        Some(&fact_subject),
        Some(&fact_predicate),
        Some(&old_fact_object),
        Some("current"),
        Some("verified"),
        Some("active"),
        Some(1_000),
        Some(1_001),
        Some(1_000),
        None,
        Some(1_002),
    )
    .await
    .expect("old server region card");

    let new_card = apply_memory_card_update(
        &client,
        "project_alpha",
        "review",
        &format!("Server region fact v2 {suffix}"),
        "Server region moved to us-east.",
        "Manual-style semantic temporal check new region.",
        &["semantic".to_string(), "temporal".to_string()],
        &json!({
            "source_event_ids": [format!("event:server-region-new:{suffix}")],
            "artifact_refs": [format!("artifact://proof/server-region-new/{suffix}")],
            "message_refs": [format!("thread:server-region-new:{suffix}")],
            "evidence_span": {"kind":"memory_card","case":"server_region_new"},
            "source_kind": "semantic_temporal_seed"
        }),
        Some(&fact_subject),
        Some(&fact_predicate),
        Some(&new_fact_object),
        Some("current"),
        Some("verified"),
        Some("active"),
        Some(2_000),
        Some(2_001),
        Some(2_000),
        None,
        Some(2_002),
    )
    .await
    .expect("new server region card");

    let project = get_project_by_code(&client, "project_alpha")
        .await
        .expect("project");
    let namespace = get_namespace_by_code(&client, project.project_id, "review")
        .await
        .expect("namespace");
    let generic_query = format!("What is the current region of {}?", fact_subject);

    let historical_hits = search_memory_cards_for_namespace(
        &client,
        project.project_id,
        namespace.namespace_id,
        &generic_query,
        10,
        Some(1_500),
    )
    .await
    .expect("generic historical hits");
    assert!(
        historical_hits
            .iter()
            .any(|card| card.memory_card_id == old_card.memory_card_id)
    );
    assert!(
        historical_hits
            .iter()
            .all(|card| card.memory_card_id != new_card.memory_card_id)
    );

    let future_hits = search_memory_cards_for_namespace(
        &client,
        project.project_id,
        namespace.namespace_id,
        &generic_query,
        10,
        Some(2_500),
    )
    .await
    .expect("generic future hits");
    assert!(
        future_hits
            .iter()
            .any(|card| card.memory_card_id == new_card.memory_card_id)
    );
    assert!(
        future_hits
            .iter()
            .all(|card| card.memory_card_id != old_card.memory_card_id)
    );
}

#[tokio::test]
async fn retracting_memory_card_closes_temporal_window_for_latest_and_future_slices() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let Some((key, value)) = trimmed.split_once('=') else {
                continue;
            };
            if std::env::var_os(key).is_none() {
                unsafe {
                    std::env::set_var(key, value);
                }
            }
        }
    }
    unsafe {
        std::env::set_var("AMI_STACK_NAME", "default");
    }
    let cfg = AppConfig::from_env().expect("env config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("unix epoch")
        .as_nanos();
    let fact_subject = format!("service.status.{suffix}");
    let fact_predicate = "deployment_state".to_string();
    let fact_object = "stable".to_string();

    let card = apply_memory_card_update(
        &client,
        "project_alpha",
        "review",
        &format!("Service status fact {suffix}"),
        "Service status was stable.",
        "Retraction temporal closure regression.",
        &["semantic".to_string(), "temporal".to_string()],
        &json!({
            "source_event_ids": [format!("event:service-status:{suffix}")],
            "artifact_refs": [format!("artifact://proof/service-status/{suffix}")],
            "message_refs": [format!("thread:service-status:{suffix}")],
            "evidence_span": {"kind":"memory_card","case":"retracted_temporal_window"},
            "source_kind": "semantic_temporal_seed"
        }),
        Some(&fact_subject),
        Some(&fact_predicate),
        Some(&fact_object),
        Some("current"),
        Some("verified"),
        Some("active"),
        Some(1_000),
        Some(1_001),
        Some(1_000),
        None,
        Some(1_002),
    )
    .await
    .expect("initial current fact");

    update_memory_card_truth_state(
        &client,
        card.memory_card_id,
        Some("retracted"),
        Some("verified"),
        Some("inactive"),
        Some(2_000),
    )
    .await
    .expect("retract fact");

    let row = client
        .query_one(
            r#"
                SELECT truth_state, status, valid_to_epoch_ms, last_verified_at_epoch_ms
                FROM ami.memory_cards
                WHERE memory_card_id = $1
                "#,
            &[&card.memory_card_id],
        )
        .await
        .expect("retracted card refresh");
    assert_eq!(row.get::<_, String>(0), "retracted");
    assert_eq!(row.get::<_, String>(1), "inactive");
    assert_eq!(row.get::<_, Option<i64>>(2), Some(2_000));
    assert_eq!(row.get::<_, Option<i64>>(3), Some(2_000));

    let project = get_project_by_code(&client, "project_alpha")
        .await
        .expect("project");
    let namespace = get_namespace_by_code(&client, project.project_id, "review")
        .await
        .expect("namespace");

    let historical_hits = search_memory_cards_for_namespace(
        &client,
        project.project_id,
        namespace.namespace_id,
        &fact_subject,
        10,
        Some(1_500),
    )
    .await
    .expect("historical retracted hits");
    assert!(
        historical_hits
            .iter()
            .any(|candidate| candidate.memory_card_id == card.memory_card_id)
    );

    let future_hits = search_memory_cards_for_namespace(
        &client,
        project.project_id,
        namespace.namespace_id,
        &fact_subject,
        10,
        Some(2_500),
    )
    .await
    .expect("future retracted hits");
    assert!(
        future_hits
            .iter()
            .all(|candidate| candidate.memory_card_id != card.memory_card_id)
    );

    let latest_hits = search_memory_cards_for_namespace(
        &client,
        project.project_id,
        namespace.namespace_id,
        &fact_subject,
        10,
        None,
    )
    .await
    .expect("latest retracted hits");
    assert!(
        latest_hits
            .iter()
            .all(|candidate| candidate.memory_card_id != card.memory_card_id)
    );
}

#[tokio::test]
async fn latest_memory_card_search_prefers_current_verified_over_conflicted_candidates() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let Some((key, value)) = trimmed.split_once('=') else {
                continue;
            };
            if std::env::var_os(key).is_none() {
                unsafe {
                    std::env::set_var(key, value);
                }
            }
        }
    }
    unsafe {
        std::env::set_var("AMI_STACK_NAME", "default");
    }
    let cfg = AppConfig::from_env().expect("env config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("unix epoch")
        .as_nanos();
    let fact_subject = format!("service.owner.{suffix}");
    let fact_predicate = "team".to_string();
    let fact_object = "platform".to_string();

    let current_verified = create_memory_card(
        &client,
        "project_alpha",
        "review",
        &format!("Service owner current verified {suffix}"),
        "Current verified ownership fact.",
        "The service owner is team platform.",
        &["semantic".to_string(), "temporal".to_string()],
        &json!({
            "source_event_ids": [format!("event:owner-current:{suffix}")],
            "artifact_refs": [format!("artifact://proof/owner-current/{suffix}")],
            "message_refs": [format!("thread:owner-current:{suffix}")],
            "evidence_span": {"kind":"memory_card","case":"current_verified_priority"},
            "source_kind": "semantic_temporal_seed"
        }),
        Some(&fact_subject),
        Some(&fact_predicate),
        Some(&fact_object),
        Some("current"),
        Some("verified"),
        Some("active"),
        Some(1_000),
        Some(1_001),
        Some(1_000),
        None,
        Some(1_002),
    )
    .await
    .expect("current verified fact");

    let conflicted_newer = create_memory_card(
            &client,
            "project_alpha",
            "review",
            &format!("Service owner conflicted {suffix}"),
            "Conflicted ownership claim with fresher timestamp.",
            "The service owner might be team platform, but this claim is conflicted and should not outrank the verified current fact.",
            &["semantic".to_string(), "temporal".to_string(), "conflict".to_string()],
            &json!({
                "source_event_ids": [format!("event:owner-conflicted:{suffix}")],
                "artifact_refs": [format!("artifact://proof/owner-conflicted/{suffix}")],
                "message_refs": [format!("thread:owner-conflicted:{suffix}")],
                "evidence_span": {"kind":"memory_card","case":"conflicted_priority"},
                "source_kind": "semantic_temporal_seed"
            }),
            Some(&fact_subject),
            Some(&fact_predicate),
            Some(&fact_object),
            Some("conflicted"),
            Some("disputed"),
            Some("active"),
            Some(2_000),
            Some(2_001),
            Some(2_000),
            None,
            Some(2_002),
        )
        .await
        .expect("conflicted newer fact");

    let hits = search_memory_cards_for_namespace(
        &client,
        get_project_by_code(&client, "project_alpha")
            .await
            .expect("project")
            .project_id,
        get_namespace_by_code(
            &client,
            get_project_by_code(&client, "project_alpha")
                .await
                .expect("project")
                .project_id,
            "review",
        )
        .await
        .expect("namespace")
        .namespace_id,
        &fact_subject,
        10,
        None,
    )
    .await
    .expect("latest owner hits");

    assert!(!hits.is_empty());
    assert_eq!(hits[0].memory_card_id, current_verified.memory_card_id);
    assert!(
        hits.iter()
            .any(|candidate| candidate.memory_card_id == conflicted_newer.memory_card_id)
    );
}

#[tokio::test]
async fn create_skill_card_candidate_surfaces_stage2_runtime_fields() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let Some((key, value)) = trimmed.split_once('=') else {
                continue;
            };
            if std::env::var_os(key).is_none() {
                unsafe {
                    std::env::set_var(key, value);
                }
            }
        }
    }
    unsafe {
        std::env::set_var("AMI_STACK_NAME", "default");
    }
    let cfg = AppConfig::from_env().expect("env config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("unix epoch")
        .as_nanos();
    let namespace_code = format!("review-stage2-runtime-{suffix}");
    ensure_project_alpha_test_namespace(&client, &namespace_code).await;
    let source_event_ids = vec![format!("event:skill:{suffix}")];
    let artifact_refs = vec![format!("artifact://proof/skill/{suffix}")];
    let evidence_span = json!({"path":"docs/skill.md","line_start":1,"line_end":2});

    let card = create_skill_card_candidate(
        &client,
        "project_alpha",
        &namespace_code,
        &format!("stage2_skill_{suffix}"),
        1,
        "Stage2 Skill",
        "Restore continuity safely",
        &["trigger".to_string()],
        &["precondition".to_string()],
        &["step one".to_string()],
        &["done".to_string()],
        &[],
        Some("expected"),
        "project_private",
        "project",
        &["codex".to_string()],
        &[],
        &[],
        &[],
        &source_event_ids,
        &artifact_refs,
        &evidence_span,
        None,
        Some("extract"),
    )
    .await
    .expect("skill card");

    assert_eq!(card.skill_candidate_class, "skill_hint");
    assert_eq!(card.skill_derivation_kind, "extract");
    assert_eq!(card.skill_source_kind.as_deref(), Some("raw_event_append"));
    assert_eq!(card.skill_evidence_span["path"], json!("docs/skill.md"));
    assert_eq!(
        card.skill_evidence_span["stage2_runtime"]["policy_and_scope_filter"]["visibility_scope"],
        json!("project_shared")
    );
    assert_eq!(
        card.skill_evidence_span["stage2_runtime"]["verification_conflict_check"]["duplicate_version_conflict"],
        json!(false)
    );
    assert!(!card.skill_hot_path_write_eligible);
    assert!(card.skill_background_consolidation_recommended);
}

#[tokio::test]
async fn create_skill_card_candidate_accepts_negative_class() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let Some((key, value)) = trimmed.split_once('=') else {
                continue;
            };
            if std::env::var_os(key).is_none() {
                unsafe {
                    std::env::set_var(key, value);
                }
            }
        }
    }
    unsafe {
        std::env::set_var("AMI_STACK_NAME", "default");
    }
    let cfg = AppConfig::from_env().expect("env config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("unix epoch")
        .as_nanos();
    let namespace_code = format!("review-stage3-negative-class-{suffix}");
    ensure_project_alpha_test_namespace(&client, &namespace_code).await;

    let card = create_skill_card_candidate(
        &client,
        "project_alpha",
        &namespace_code,
        &format!("stage3a_failure_pattern_{suffix}"),
        1,
        "Failure pattern",
        "Avoid a known failure mode",
        &["avoid repeating failure".to_string()],
        &["context present".to_string()],
        &["check invariant".to_string()],
        &["no failure observed".to_string()],
        &["failure likely".to_string()],
        Some("stable recovery"),
        "project_private",
        "project",
        &["codex".to_string()],
        &[],
        &[],
        &[],
        &[format!("event:failure:{suffix}")],
        &[format!("artifact://proof/failure/{suffix}")],
        &json!({"path":"docs/AMAI_GLOBAL_MEMORY_ROADMAP.md","line_start":1,"line_end":3}),
        Some("failure_pattern"),
        Some("extract"),
    )
    .await
    .expect("negative class skill card");

    assert_eq!(card.skill_candidate_class, "failure_pattern");
}

#[tokio::test]
async fn create_skill_card_candidate_rejects_duplicate_skill_version() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let Some((key, value)) = trimmed.split_once('=') else {
                continue;
            };
            if std::env::var_os(key).is_none() {
                unsafe {
                    std::env::set_var(key, value);
                }
            }
        }
    }
    unsafe {
        std::env::set_var("AMI_STACK_NAME", "default");
    }
    let cfg = AppConfig::from_env().expect("env config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("unix epoch")
        .as_nanos();
    let namespace_code = format!("review-stage2-dup-{suffix}");
    ensure_project_alpha_test_namespace(&client, &namespace_code).await;
    let skill_id = format!("stage2_skill_duplicate_{suffix}");

    create_skill_card_candidate(
        &client,
        "project_alpha",
        &namespace_code,
        &skill_id,
        1,
        "Stage2 Skill",
        "Restore continuity safely",
        &["trigger".to_string()],
        &["precondition".to_string()],
        &["step one".to_string()],
        &["done".to_string()],
        &[],
        Some("expected"),
        "project_private",
        "project",
        &["codex".to_string()],
        &[],
        &[],
        &[],
        &[format!("event:skill:first:{suffix}")],
        &[format!("artifact://proof/skill/first/{suffix}")],
        &json!({"path":"docs/skill.md","line_start":1,"line_end":2}),
        None,
        Some("extract"),
    )
    .await
    .expect("first skill card");

    let error = create_skill_card_candidate(
        &client,
        "project_alpha",
        &namespace_code,
        &skill_id,
        1,
        "Stage2 Skill",
        "Restore continuity safely",
        &["trigger".to_string()],
        &["precondition".to_string()],
        &["step one".to_string()],
        &["done".to_string()],
        &[],
        Some("expected"),
        "project_private",
        "project",
        &["codex".to_string()],
        &[],
        &[],
        &[],
        &[format!("event:skill:second:{suffix}")],
        &[format!("artifact://proof/skill/second/{suffix}")],
        &json!({"path":"docs/skill.md","line_start":3,"line_end":4}),
        None,
        Some("extract"),
    )
    .await
    .expect_err("duplicate skill version rejected");
    assert!(
        error
            .to_string()
            .contains("existing skill_id/version truth in the same namespace")
    );
}

#[tokio::test]
async fn create_skill_card_candidate_rejects_similar_skill_without_refinement_action() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let Some((key, value)) = trimmed.split_once('=') else {
                continue;
            };
            if std::env::var_os(key).is_none() {
                unsafe {
                    std::env::set_var(key, value);
                }
            }
        }
    }
    unsafe {
        std::env::set_var("AMI_STACK_NAME", "default");
    }
    let cfg = AppConfig::from_env().expect("env config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("unix epoch")
        .as_nanos();
    let namespace_code = format!("review-stage3-reject-similar-{suffix}");
    ensure_project_alpha_test_namespace(&client, &namespace_code).await;

    create_skill_card_candidate(
        &client,
        "project_alpha",
        &namespace_code,
        &format!("stage3_patch_base_{suffix}"),
        1,
        "Continuity Restore Skill",
        "Restore continuity safely",
        &["restore continuity".to_string()],
        &["continuity fresh".to_string()],
        &["inspect startup gate".to_string()],
        &["required return cleared".to_string()],
        &["continuity stale".to_string()],
        Some("resume restored"),
        "project_private",
        "project",
        &["codex".to_string()],
        &["gpt-5".to_string()],
        &["exec_command".to_string()],
        &["continuity".to_string()],
        &[format!("event:stage3:patch-base:{suffix}")],
        &[format!("artifact://proof/stage3/patch-base/{suffix}")],
        &json!({"path":"docs/AMAI_GLOBAL_MEMORY_ROADMAP.md","line_start":1217,"line_end":1221}),
        None,
        Some("extract"),
    )
    .await
    .expect("base skill");

    let error = create_skill_card_candidate(
        &client,
        "project_alpha",
        &namespace_code,
        &format!("stage3_patch_clone_{suffix}"),
        1,
        "Continuity Restore Skill",
        "Restore continuity safely",
        &["restore continuity".to_string()],
        &["continuity fresh".to_string()],
        &["inspect startup gate".to_string()],
        &["required return cleared".to_string()],
        &["continuity stale".to_string()],
        Some("resume restored"),
        "project_private",
        "project",
        &["codex".to_string()],
        &["gpt-5".to_string()],
        &["exec_command".to_string()],
        &["continuity".to_string()],
        &[format!("event:stage3:patch-clone:{suffix}")],
        &[format!("artifact://proof/stage3/patch-clone/{suffix}")],
        &json!({"path":"docs/AMAI_GLOBAL_MEMORY_ROADMAP.md","line_start":1217,"line_end":1221}),
        None,
        Some("extract"),
    )
    .await
    .expect_err("similar skill without refinement action rejected");
    assert!(error.to_string().contains("similar skill already exists"));
}

#[tokio::test]
async fn create_skill_card_candidate_patch_links_parent_and_merge_group() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let Some((key, value)) = trimmed.split_once('=') else {
                continue;
            };
            if std::env::var_os(key).is_none() {
                unsafe {
                    std::env::set_var(key, value);
                }
            }
        }
    }
    unsafe {
        std::env::set_var("AMI_STACK_NAME", "default");
    }
    let cfg = AppConfig::from_env().expect("env config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("unix epoch")
        .as_nanos();
    let namespace_code = format!("review-stage3-patch-{suffix}");
    ensure_project_alpha_test_namespace(&client, &namespace_code).await;
    let skill_id = format!("stage3_patch_skill_{suffix}");

    let base = create_skill_card_candidate(
        &client,
        "project_alpha",
        &namespace_code,
        &skill_id,
        1,
        "Continuity Restore Skill",
        "Restore continuity safely",
        &["restore continuity".to_string()],
        &["continuity fresh".to_string()],
        &["inspect startup gate".to_string()],
        &["required return cleared".to_string()],
        &["continuity stale".to_string()],
        Some("resume restored"),
        "project_private",
        "project",
        &["codex".to_string()],
        &["gpt-5".to_string()],
        &["exec_command".to_string()],
        &["continuity".to_string()],
        &[format!("event:stage3:patch-base:{suffix}")],
        &[format!("artifact://proof/stage3/patch-base/{suffix}")],
        &json!({"path":"docs/AMAI_GLOBAL_MEMORY_ROADMAP.md","line_start":1217,"line_end":1221}),
        None,
        Some("extract"),
    )
    .await
    .expect("base skill");

    let patch = super::create_skill_card_candidate_with_refinement(
        &client,
        "project_alpha",
        &namespace_code,
        &skill_id,
        2,
        "Continuity Restore Skill",
        "Restore continuity safely",
        &["restore continuity".to_string()],
        &["continuity fresh".to_string()],
        &[
            "inspect startup gate".to_string(),
            "confirm startup next action".to_string(),
        ],
        &["required return cleared".to_string()],
        &["continuity stale".to_string()],
        Some("resume restored"),
        "project_private",
        "project",
        &["codex".to_string()],
        &["gpt-5".to_string()],
        &["exec_command".to_string()],
        &["continuity".to_string()],
        &[format!("event:stage3:patch-child:{suffix}")],
        &[format!("artifact://proof/stage3/patch-child/{suffix}")],
        &json!({"path":"docs/AMAI_GLOBAL_MEMORY_ROADMAP.md","line_start":1217,"line_end":1221}),
        None,
        Some("patch"),
        Some(base.skill_card_id),
        None,
        Some("extract"),
    )
    .await
    .expect("patch skill");
    assert_eq!(patch.skill_patch_parent_id, Some(base.skill_card_id));
    assert_eq!(patch.skill_merge_group_id, Some(base.skill_card_id));
    assert_eq!(patch.skill_id, skill_id);
    assert_eq!(patch.skill_version, 2);
}

#[tokio::test]
async fn create_skill_card_candidate_allows_explicit_new_despite_similarity() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let Some((key, value)) = trimmed.split_once('=') else {
                continue;
            };
            if std::env::var_os(key).is_none() {
                unsafe {
                    std::env::set_var(key, value);
                }
            }
        }
    }
    unsafe {
        std::env::set_var("AMI_STACK_NAME", "default");
    }
    let cfg = AppConfig::from_env().expect("env config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("unix epoch")
        .as_nanos();
    let namespace_code = format!("review-stage3-explicit-new-{suffix}");
    ensure_project_alpha_test_namespace(&client, &namespace_code).await;

    let base = create_skill_card_candidate(
        &client,
        "project_alpha",
        &namespace_code,
        &format!("stage3_new_base_{suffix}"),
        1,
        "Continuity Restore Skill",
        "Restore continuity safely",
        &["restore continuity".to_string()],
        &["continuity fresh".to_string()],
        &["inspect startup gate".to_string()],
        &["required return cleared".to_string()],
        &["continuity stale".to_string()],
        Some("resume restored"),
        "project_private",
        "project",
        &["codex".to_string()],
        &["gpt-5".to_string()],
        &["exec_command".to_string()],
        &["continuity".to_string()],
        &[format!("event:stage3:new-base:{suffix}")],
        &[format!("artifact://proof/stage3/new-base/{suffix}")],
        &json!({"path":"docs/AMAI_GLOBAL_MEMORY_ROADMAP.md","line_start":1217,"line_end":1221}),
        None,
        Some("extract"),
    )
    .await
    .expect("base skill");

    let explicit_new = super::create_skill_card_candidate_with_refinement(
        &client,
        "project_alpha",
        &namespace_code,
        &format!("stage3_new_explicit_{suffix}"),
        1,
        "Continuity Restore Skill",
        "Restore continuity safely",
        &["restore continuity".to_string()],
        &["continuity fresh".to_string()],
        &["inspect startup gate".to_string()],
        &["required return cleared".to_string()],
        &["continuity stale".to_string()],
        Some("resume restored"),
        "project_private",
        "project",
        &["codex".to_string()],
        &["gpt-5".to_string()],
        &["exec_command".to_string()],
        &["continuity".to_string()],
        &[format!("event:stage3:new-explicit:{suffix}")],
        &[format!("artifact://proof/stage3/new-explicit/{suffix}")],
        &json!({"path":"docs/AMAI_GLOBAL_MEMORY_ROADMAP.md","line_start":1217,"line_end":1221}),
        None,
        Some("new"),
        None,
        None,
        Some("extract"),
    )
    .await
    .expect("explicit new skill");

    assert_ne!(explicit_new.skill_card_id, base.skill_card_id);
    assert_eq!(explicit_new.skill_patch_parent_id, None);
    assert_eq!(explicit_new.skill_merge_group_id, None);
    assert_eq!(
        explicit_new.skill_evidence_span["skill_refinement_decision"]["action"],
        json!("new")
    );
    assert_eq!(
        explicit_new.skill_evidence_span["skill_refinement_decision"]["similarity_required_decision"],
        json!(true)
    );
}

#[tokio::test]
async fn build_skill_review_payload_surfaces_version_history_with_actor_and_reason() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let Some((key, value)) = trimmed.split_once('=') else {
                continue;
            };
            if std::env::var_os(key).is_none() {
                unsafe {
                    std::env::set_var(key, value);
                }
            }
        }
    }
    unsafe {
        std::env::set_var("AMI_STACK_NAME", "default");
    }
    let cfg = AppConfig::from_env().expect("env config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("unix epoch")
        .as_nanos();
    let namespace_code = format!("review-stage3-history-{suffix}");
    ensure_project_alpha_test_namespace(&client, &namespace_code).await;
    let skill_id = format!("stage3_history_skill_{suffix}");

    let base = create_skill_card_candidate(
        &client,
        "project_alpha",
        &namespace_code,
        &skill_id,
        1,
        "Continuity Restore Skill",
        "Restore continuity safely",
        &["restore continuity".to_string()],
        &["continuity fresh".to_string()],
        &["inspect startup gate".to_string()],
        &["required return cleared".to_string()],
        &["continuity stale".to_string()],
        Some("resume restored"),
        "project_private",
        "project",
        &["codex".to_string()],
        &["gpt-5".to_string()],
        &["exec_command".to_string()],
        &["continuity".to_string()],
        &[format!("event:stage3:history-base:{suffix}")],
        &[format!("artifact://proof/stage3/history-base/{suffix}")],
        &json!({
            "skill_change_summary": {
                "changed_by": "seed-evaluator",
                "change_reason": "initial extraction"
            }
        }),
        None,
        Some("extract"),
    )
    .await
    .expect("base skill");

    let patch = super::create_skill_card_candidate_with_refinement(
        &client,
        "project_alpha",
        &namespace_code,
        &skill_id,
        2,
        "Continuity Restore Skill",
        "Restore continuity safely",
        &["restore continuity".to_string()],
        &["continuity fresh".to_string()],
        &[
            "inspect startup gate".to_string(),
            "confirm startup next action".to_string(),
        ],
        &["required return cleared".to_string()],
        &["continuity stale".to_string()],
        Some("resume restored"),
        "project_private",
        "project",
        &["codex".to_string()],
        &["gpt-5".to_string()],
        &["exec_command".to_string()],
        &["continuity".to_string()],
        &[format!("event:stage3:history-patch:{suffix}")],
        &[format!("artifact://proof/stage3/history-patch/{suffix}")],
        &json!({
            "skill_change_summary": {
                "changed_by": "reviewer-1",
                "change_reason": "added explicit startup-next-action confirmation"
            }
        }),
        None,
        Some("patch"),
        Some(base.skill_card_id),
        None,
        Some("extract"),
    )
    .await
    .expect("patch skill");

    let review = super::build_skill_review_payload(&client, patch.skill_card_id)
        .await
        .expect("review payload");
    let history = review["history"].as_array().expect("history array");
    assert_eq!(history.len(), 2);
    assert_eq!(history[0]["skill_version"], json!(1));
    assert_eq!(history[0]["changed_by"], json!("seed-evaluator"));
    assert_eq!(history[0]["change_reason"], json!("initial extraction"));
    assert_eq!(history[1]["skill_version"], json!(2));
    assert_eq!(history[1]["changed_by"], json!("reviewer-1"));
    assert_eq!(
        history[1]["change_reason"],
        json!("added explicit startup-next-action confirmation")
    );
    assert_eq!(history[1]["refinement_action"], json!("patch"));
    assert_eq!(
        history[1]["skill_patch_parent_id"],
        json!(base.skill_card_id)
    );
}

#[tokio::test]
async fn build_skill_review_payload_surfaces_merge_history_and_group_lineage() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let Some((key, value)) = trimmed.split_once('=') else {
                continue;
            };
            if std::env::var_os(key).is_none() {
                unsafe {
                    std::env::set_var(key, value);
                }
            }
        }
    }
    unsafe {
        std::env::set_var("AMI_STACK_NAME", "default");
    }
    let cfg = AppConfig::from_env().expect("env config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("unix epoch")
        .as_nanos();
    let namespace_code = format!("review-stage3-history-merge-{suffix}");
    ensure_project_alpha_test_namespace(&client, &namespace_code).await;

    let base = create_skill_card_candidate(
        &client,
        "project_alpha",
        &namespace_code,
        &format!("stage3_history_merge_base_{suffix}"),
        1,
        "Continuity Restore Skill",
        "Restore continuity safely",
        &["restore continuity".to_string()],
        &["continuity fresh".to_string()],
        &["inspect startup gate".to_string()],
        &["required return cleared".to_string()],
        &["continuity stale".to_string()],
        Some("resume restored"),
        "project_private",
        "project",
        &["codex".to_string()],
        &["gpt-5".to_string()],
        &["exec_command".to_string()],
        &["continuity".to_string()],
        &[format!("event:stage3:history-merge-base:{suffix}")],
        &[format!(
            "artifact://proof/stage3/history-merge-base/{suffix}"
        )],
        &json!({
            "skill_change_summary": {
                "changed_by": "seed-evaluator",
                "change_reason": "base lineage"
            }
        }),
        None,
        Some("extract"),
    )
    .await
    .expect("base skill");

    let merged = super::create_skill_card_candidate_with_refinement(
        &client,
        "project_alpha",
        &namespace_code,
        &format!("stage3_history_merge_peer_{suffix}"),
        1,
        "Continuity Restore Skill",
        "Restore continuity safely",
        &["restore continuity".to_string()],
        &["continuity fresh".to_string()],
        &[
            "inspect startup gate".to_string(),
            "confirm startup next action".to_string(),
        ],
        &["required return cleared".to_string()],
        &["continuity stale".to_string()],
        Some("resume restored"),
        "project_private",
        "project",
        &["codex".to_string()],
        &["gpt-5".to_string()],
        &["exec_command".to_string()],
        &["continuity".to_string()],
        &[format!("event:stage3:history-merge-peer:{suffix}")],
        &[format!(
            "artifact://proof/stage3/history-merge-peer/{suffix}"
        )],
        &json!({
            "skill_change_summary": {
                "changed_by": "reviewer-merge",
                "change_reason": "merged overlapping restore variant"
            }
        }),
        None,
        Some("merge"),
        None,
        None,
        Some("extract"),
    )
    .await
    .expect("merge skill");

    let review = super::build_skill_review_payload(&client, merged.skill_card_id)
        .await
        .expect("merge review");
    let history = review["history"].as_array().expect("history array");
    assert_eq!(history.len(), 2);
    assert!(history.iter().any(|entry| {
        entry["skill_card_id"] == json!(base.skill_card_id)
            && entry["changed_by"] == json!("seed-evaluator")
            && entry["change_reason"] == json!("base lineage")
    }));
    assert!(history.iter().any(|entry| {
        entry["skill_card_id"] == json!(merged.skill_card_id)
            && entry["changed_by"] == json!("reviewer-merge")
            && entry["change_reason"] == json!("merged overlapping restore variant")
            && entry["refinement_action"] == json!("merge")
            && entry["skill_merge_group_id"] == json!(base.skill_card_id)
    }));
}

#[tokio::test]
async fn build_skill_review_payload_keeps_history_after_promote_eval_and_reuse() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let Some((key, value)) = trimmed.split_once('=') else {
                continue;
            };
            if std::env::var_os(key).is_none() {
                unsafe {
                    std::env::set_var(key, value);
                }
            }
        }
    }
    unsafe {
        std::env::set_var("AMI_STACK_NAME", "default");
    }
    let cfg = AppConfig::from_env().expect("env config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("unix epoch")
        .as_nanos();
    let namespace_code = format!("review-stage3-history-lifecycle-{suffix}");
    ensure_project_alpha_test_namespace(&client, &namespace_code).await;
    let skill_id = format!("stage3_history_lifecycle_{suffix}");

    let base = create_skill_card_candidate(
        &client,
        "project_alpha",
        &namespace_code,
        &skill_id,
        1,
        "Continuity Restore Skill",
        "Restore continuity safely",
        &["restore continuity".to_string()],
        &["continuity fresh".to_string()],
        &["inspect startup gate".to_string()],
        &["required return cleared".to_string()],
        &["continuity stale".to_string()],
        Some("resume restored"),
        "project_private",
        "project",
        &["codex".to_string()],
        &["gpt-5".to_string()],
        &["exec_command".to_string()],
        &["continuity".to_string()],
        &[format!("event:stage3:history-life-base:{suffix}")],
        &[format!(
            "artifact://proof/stage3/history-life-base/{suffix}"
        )],
        &json!({
            "skill_change_summary": {
                "changed_by": "seed-evaluator",
                "change_reason": "initial extraction"
            }
        }),
        None,
        Some("extract"),
    )
    .await
    .expect("base skill");

    let patch = super::create_skill_card_candidate_with_refinement(
        &client,
        "project_alpha",
        &namespace_code,
        &skill_id,
        2,
        "Continuity Restore Skill",
        "Restore continuity safely",
        &["restore continuity".to_string()],
        &["continuity fresh".to_string()],
        &[
            "inspect startup gate".to_string(),
            "confirm startup next action".to_string(),
        ],
        &["required return cleared".to_string()],
        &["continuity stale".to_string()],
        Some("resume restored"),
        "project_private",
        "project",
        &["codex".to_string()],
        &["gpt-5".to_string()],
        &["exec_command".to_string()],
        &["continuity".to_string()],
        &[format!("event:stage3:history-life-patch:{suffix}")],
        &[format!(
            "artifact://proof/stage3/history-life-patch/{suffix}"
        )],
        &json!({
            "skill_change_summary": {
                "changed_by": "reviewer-1",
                "change_reason": "added explicit startup-next-action confirmation"
            }
        }),
        None,
        Some("patch"),
        Some(base.skill_card_id),
        None,
        Some("extract"),
    )
    .await
    .expect("patch skill");

    let message_refs = json!([format!("thread:stage3-history-lifecycle:{suffix}")]);
    create_skill_evidence_bundle(
        &client,
        patch.skill_card_id,
        "trace",
        Some("manual evidence"),
        &[format!("event:stage3:history-life-evidence:{suffix}")],
        &[format!(
            "artifact://proof/stage3/history-life-evidence/{suffix}"
        )],
        Some("manual_proof"),
        Some(&message_refs),
        Some(&json!({"kind":"bundle","context":"continuity"})),
        Some("extract"),
        Some("skill-evidence-bundle-envelope-v1"),
    )
    .await
    .expect("evidence");
    record_skill_trigger_match(
        &client,
        patch.skill_card_id,
        "project_task",
        "restore continuity",
        true,
        Some("trigger matched"),
        Some("manual_trigger"),
        Some(&json!([format!(
            "event:stage3:history-life-trigger:{suffix}"
        )])),
        Some(&json!([format!(
            "artifact://proof/stage3/history-life-trigger/{suffix}"
        )])),
        Some(&message_refs),
        Some(&json!({"matched":true,"context":"continuity"})),
        Some("extract"),
        Some("skill-trigger-match-envelope-v1"),
    )
    .await
    .expect("trigger");
    record_skill_trial_run(
        &client,
        patch.skill_card_id,
        "shadow",
        Some("manual shadow"),
        Some("codex"),
        Some("gpt-5"),
        Some("exec_command"),
        true,
        false,
        "success",
        Some("shadow success"),
        Some("manual_shadow"),
        Some(&json!([format!(
            "event:stage3:history-life-shadow:{suffix}"
        )])),
        Some(&json!([format!(
            "artifact://proof/stage3/history-life-shadow/{suffix}"
        )])),
        Some(&message_refs),
        Some(&json!({"matched":true,"applied":false,"context":"continuity"})),
        Some("extract"),
        Some("skill-trial-run-envelope-v1"),
    )
    .await
    .expect("shadow run");
    record_skill_eval(
        &client,
        patch.skill_card_id,
        "promote_shadow",
        "manual_eval",
        true,
        true,
        true,
        0.0,
        Some("promote to shadow"),
        Some("manual_eval"),
        Some(&json!([format!(
            "event:stage3:history-life-eval-shadow:{suffix}"
        )])),
        Some(&json!([format!(
            "artifact://proof/stage3/history-life-eval-shadow/{suffix}"
        )])),
        Some(&message_refs),
        Some(&json!({"kind":"eval","phase":"shadow"})),
        Some("extract"),
        Some("skill-eval-envelope-v1"),
    )
    .await
    .expect("promote shadow");
    record_skill_trial_run(
        &client,
        patch.skill_card_id,
        "trial",
        Some("manual trial"),
        Some("codex"),
        Some("gpt-5"),
        Some("exec_command"),
        true,
        true,
        "success",
        Some("trial success"),
        Some("manual_trial"),
        Some(&json!([format!(
            "event:stage3:history-life-trial:{suffix}"
        )])),
        Some(&json!([format!(
            "artifact://proof/stage3/history-life-trial/{suffix}"
        )])),
        Some(&message_refs),
        Some(&json!({"matched":true,"applied":true,"context":"continuity"})),
        Some("extract"),
        Some("skill-trial-run-envelope-v1"),
    )
    .await
    .expect("trial run");
    record_skill_eval(
        &client,
        patch.skill_card_id,
        "promote_trial",
        "manual_eval",
        true,
        true,
        true,
        0.2,
        Some("promote to trial"),
        Some("manual_eval"),
        Some(&json!([format!(
            "event:stage3:history-life-eval-trial:{suffix}"
        )])),
        Some(&json!([format!(
            "artifact://proof/stage3/history-life-eval-trial/{suffix}"
        )])),
        Some(&message_refs),
        Some(&json!({"kind":"eval","phase":"trial"})),
        Some("extract"),
        Some("skill-eval-envelope-v1"),
    )
    .await
    .expect("promote trial");
    record_skill_reuse_log(
        &client,
        patch.skill_card_id,
        "trial",
        Some("manual reuse"),
        "success",
        Some("reused successfully"),
        &[format!("event:stage3:history-life-reuse:{suffix}")],
        &[format!(
            "artifact://proof/stage3/history-life-reuse/{suffix}"
        )],
        Some("manual_reuse"),
        Some(&message_refs),
        Some(&json!({
            "matched":true,
            "applied":true,
            "context":"continuity",
            "runtime":"codex",
            "model":"gpt-5",
            "tool":"exec_command"
        })),
        Some("extract"),
        Some("skill-reuse-log-envelope-v1"),
    )
    .await
    .expect("reuse log");

    let review = super::build_skill_review_payload(&client, patch.skill_card_id)
        .await
        .expect("review payload");
    let history = review["history"].as_array().expect("history array");
    assert_eq!(history.len(), 2);
    assert_eq!(history[0]["changed_by"], json!("seed-evaluator"));
    assert_eq!(history[1]["changed_by"], json!("reviewer-1"));
    assert_eq!(
        history[1]["change_reason"],
        json!("added explicit startup-next-action confirmation")
    );
    assert_eq!(review["skill"]["skill_trust_state"], json!("trial"));
    assert_eq!(review["evals"].as_array().expect("evals").len(), 2);
    assert_eq!(
        review["reuse_logs"].as_array().expect("reuse_logs").len(),
        1
    );
}

#[tokio::test]
async fn build_skill_execution_cards_filters_by_context_and_ranks_by_utility() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let Some((key, value)) = trimmed.split_once('=') else {
                continue;
            };
            if std::env::var_os(key).is_none() {
                unsafe {
                    std::env::set_var(key, value);
                }
            }
        }
    }
    unsafe {
        std::env::set_var("AMI_STACK_NAME", "default");
    }
    let cfg = AppConfig::from_env().expect("env config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("unix epoch")
        .as_nanos();
    let namespace_code = format!("review-stage3-execution-card-{suffix}");
    ensure_project_alpha_test_namespace(&client, &namespace_code).await;

    async fn promote_trial_skill(
        client: &Client,
        namespace_code: &str,
        suffix: u128,
        label: &str,
        utility_delta: f64,
        context_constraints: &[String],
        trigger_conditions: &[String],
    ) -> SkillCardRecord {
        let skill = super::create_skill_card_candidate_with_refinement(
            client,
            "project_alpha",
            namespace_code,
            &format!("stage3a_execution_card_{label}_{suffix}"),
            1,
            &format!("Stage3A {label}"),
            "Restore continuity safely",
            trigger_conditions,
            &["continuity fresh".to_string()],
            &["inspect startup gate".to_string()],
            &["required return cleared".to_string()],
            &["continuity stale".to_string()],
            Some("safe resume"),
            "project_private",
            "project",
            &["codex".to_string()],
            &["gpt-5".to_string()],
            &["exec_command".to_string()],
            context_constraints,
            &[format!("event:stage3a:{label}:{suffix}")],
            &[format!("artifact://stage3a/{label}/{suffix}")],
            &json!({"path":"docs/AMAI_GLOBAL_MEMORY_ROADMAP.md","line_start":1,"line_end":3}),
            None,
            Some("new"),
            None,
            None,
            Some("extract"),
        )
        .await
        .expect("skill candidate");

        let message_refs = json!([format!("thread:stage3a:{label}:{suffix}")]);
        let trial_context = context_constraints
            .first()
            .map(|value| value.as_str())
            .unwrap_or("continuity");
        create_skill_evidence_bundle(
            client,
            skill.skill_card_id,
            "trace",
            Some("manual evidence"),
            &[format!("event:stage3a:evidence:{label}:{suffix}")],
            &[format!("artifact://stage3a/evidence/{label}/{suffix}")],
            Some("manual_proof"),
            Some(&message_refs),
            Some(&json!({"kind":"bundle","label":label})),
            Some("extract"),
            Some("skill-evidence-bundle-envelope-v1"),
        )
        .await
        .expect("evidence");
        record_skill_trigger_match(
            client,
            skill.skill_card_id,
            "project_task",
            &trigger_conditions[0],
            true,
            Some("trigger matched"),
            Some("manual_trigger"),
            Some(&json!([format!("event:stage3a:trigger:{label}:{suffix}")])),
            Some(&json!([format!(
                "artifact://stage3a/trigger/{label}/{suffix}"
            )])),
            Some(&message_refs),
            Some(&json!({"kind":"trigger","label":label,"context":trial_context})),
            Some("extract"),
            Some("skill-trigger-match-envelope-v1"),
        )
        .await
        .expect("trigger");
        record_skill_trial_run(
            client,
            skill.skill_card_id,
            "shadow",
            Some("manual shadow"),
            Some("codex"),
            Some("gpt-5"),
            Some("exec_command"),
            true,
            false,
            "success",
            Some("shadow success"),
            Some("manual_shadow"),
            Some(&json!([format!("event:stage3a:shadow:{label}:{suffix}")])),
            Some(&json!([format!(
                "artifact://stage3a/shadow/{label}/{suffix}"
            )])),
            Some(&message_refs),
            Some(&json!({"kind":"shadow","label":label,"context":trial_context})),
            Some("extract"),
            Some("skill-trial-run-envelope-v1"),
        )
        .await
        .expect("shadow run");
        record_skill_eval(
            client,
            skill.skill_card_id,
            "promote_shadow",
            "manual_eval",
            true,
            true,
            true,
            0.0,
            Some("promote to shadow"),
            Some("manual_eval"),
            Some(&json!([format!(
                "event:stage3a:eval-shadow:{label}:{suffix}"
            )])),
            Some(&json!([format!(
                "artifact://stage3a/eval-shadow/{label}/{suffix}"
            )])),
            Some(&message_refs),
            Some(&json!({"kind":"eval","phase":"shadow","label":label})),
            Some("extract"),
            Some("skill-eval-envelope-v1"),
        )
        .await
        .expect("promote shadow");
        record_skill_trial_run(
            client,
            skill.skill_card_id,
            "trial",
            Some("manual trial"),
            Some("codex"),
            Some("gpt-5"),
            Some("exec_command"),
            true,
            true,
            "success",
            Some("trial success"),
            Some("manual_trial"),
            Some(&json!([format!("event:stage3a:trial:{label}:{suffix}")])),
            Some(&json!([format!(
                "artifact://stage3a/trial/{label}/{suffix}"
            )])),
            Some(&message_refs),
            Some(&json!({"kind":"trial","label":label,"context":trial_context})),
            Some("extract"),
            Some("skill-trial-run-envelope-v1"),
        )
        .await
        .expect("trial run");
        record_skill_eval(
            client,
            skill.skill_card_id,
            "promote_trial",
            "manual_eval",
            true,
            true,
            true,
            utility_delta,
            Some("promote to trial"),
            Some("manual_eval"),
            Some(&json!([format!("event:stage3a:eval:{label}:{suffix}")])),
            Some(&json!([format!(
                "artifact://stage3a/eval/{label}/{suffix}"
            )])),
            Some(&message_refs),
            Some(&json!({"kind":"eval","label":label})),
            Some("extract"),
            Some("skill-eval-envelope-v1"),
        )
        .await
        .expect("promote trial");
        list_skill_cards(
            client,
            Some("project_alpha"),
            Some(namespace_code),
            Some(&skill.skill_id),
        )
        .await
        .expect("list skill cards")
        .into_iter()
        .find(|card| card.skill_card_id == skill.skill_card_id)
        .expect("reloaded skill card")
    }

    let restore_card = promote_trial_skill(
        &client,
        &namespace_code,
        suffix,
        "restore",
        0.4,
        &["restore".to_string(), "continuity".to_string()],
        &["manual restore required".to_string()],
    )
    .await;
    let deploy_card = promote_trial_skill(
        &client,
        &namespace_code,
        suffix,
        "deploy",
        1.3,
        &["deploy".to_string()],
        &["manual deploy required".to_string()],
    )
    .await;

    let filtered = build_skill_execution_cards(
        &client,
        "project_alpha",
        &namespace_code,
        Some("restore"),
        Some("codex"),
        Some("gpt-5"),
        Some("exec_command"),
        true,
        false,
        false,
    )
    .await
    .expect("filtered execution cards");
    let filtered = filtered.as_array().expect("array");
    assert_eq!(filtered.len(), 1);
    assert_eq!(
        filtered[0]["skill_card_id"],
        json!(restore_card.skill_card_id)
    );
    assert_eq!(
        filtered[0]["skill_trigger_conditions"],
        json!(["manual restore required"])
    );
    assert_eq!(filtered[0]["skill_scope_type"], json!("project_private"));
    assert_eq!(filtered[0]["skill_owner_scope"], json!("project"));

    let missing_runtime = build_skill_execution_cards(
        &client,
        "project_alpha",
        &namespace_code,
        None,
        None,
        None,
        None,
        true,
        false,
        false,
    )
    .await
    .expect("missing runtime execution cards");
    let missing_runtime = missing_runtime.as_array().expect("array");
    assert!(missing_runtime.is_empty());

    let ranked = build_skill_execution_cards(
        &client,
        "project_alpha",
        &namespace_code,
        None,
        Some("codex"),
        Some("gpt-5"),
        Some("exec_command"),
        true,
        false,
        false,
    )
    .await
    .expect("ranked execution cards");
    let ranked = ranked.as_array().expect("array");
    assert!(ranked.len() >= 2);
    assert_eq!(ranked[0]["skill_card_id"], json!(deploy_card.skill_card_id));
    assert_eq!(
        ranked[1]["skill_card_id"],
        json!(restore_card.skill_card_id)
    );
}

#[tokio::test]
async fn build_skill_execution_cards_keeps_negative_procedural_classes_alongside_success() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let Some((key, value)) = trimmed.split_once('=') else {
                continue;
            };
            if std::env::var_os(key).is_none() {
                unsafe {
                    std::env::set_var(key, value);
                }
            }
        }
    }
    unsafe {
        std::env::set_var("AMI_STACK_NAME", "default");
    }
    let cfg = AppConfig::from_env().expect("env config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("unix epoch")
        .as_nanos();

    async fn promote_verified_skill(
        client: &Client,
        suffix: u128,
        label: &str,
        candidate_class: &str,
    ) -> SkillCardRecord {
        let skill = super::create_skill_card_candidate_with_refinement(
            client,
            "project_alpha",
            "review",
            &format!("stage3a_negative_execution_card_{label}_{suffix}"),
            1,
            &format!("Stage3A {label}"),
            "Surface procedural object on execution card",
            &[format!("trigger {label}")],
            &["continuity fresh".to_string()],
            &[format!("step {label}")],
            &[format!("stop {label}")],
            &[format!("forbidden {label}")],
            Some("proof"),
            "project_private",
            "project",
            &["codex".to_string()],
            &["gpt-5".to_string()],
            &["exec_command".to_string()],
            &["continuity".to_string()],
            &[format!("event:stage3a-negative:{label}:{suffix}")],
            &[format!("artifact://stage3a-negative/{label}/{suffix}")],
            &json!({"path":"docs/AMAI_GLOBAL_MEMORY_ROADMAP.md","line_start":1213,"line_end":1215}),
            Some(candidate_class),
            Some("new"),
            None,
            None,
            Some("extract"),
        )
        .await
        .expect("skill candidate");

        let message_refs = json!([format!("thread:stage3a-negative:{label}:{suffix}")]);
        create_skill_evidence_bundle(
            client,
            skill.skill_card_id,
            "trace",
            Some("manual evidence"),
            &[format!("event:stage3a-negative:evidence:{label}:{suffix}")],
            &[format!(
                "artifact://stage3a-negative/evidence/{label}/{suffix}"
            )],
            Some("manual_proof"),
            Some(&message_refs),
            Some(&json!({"kind":"bundle","label":label,"candidate_class":candidate_class})),
            Some("extract"),
            Some("skill-evidence-bundle-envelope-v1"),
        )
        .await
        .expect("evidence");
        record_skill_trigger_match(
            client,
            skill.skill_card_id,
            "project_task",
            &format!("trigger {label}"),
            true,
            Some("trigger matched"),
            Some("manual_trigger"),
            Some(&json!([format!(
                "event:stage3a-negative:trigger:{label}:{suffix}"
            )])),
            Some(&json!([format!(
                "artifact://stage3a-negative/trigger/{label}/{suffix}"
            )])),
            Some(&message_refs),
            Some(&json!({"kind":"trigger","label":label,"candidate_class":candidate_class})),
            Some("extract"),
            Some("skill-trigger-match-envelope-v1"),
        )
        .await
        .expect("trigger");
        record_skill_trial_run(
                client,
                skill.skill_card_id,
                "shadow",
                Some("manual shadow"),
                Some("codex"),
                Some("gpt-5"),
                Some("exec_command"),
                true,
                false,
                "success",
                Some("shadow success"),
                Some("manual_shadow"),
                Some(&json!([format!("event:stage3a-negative:shadow:{label}:{suffix}")])),
                Some(&json!([format!(
                    "artifact://stage3a-negative/shadow/{label}/{suffix}"
                )])),
                Some(&message_refs),
                Some(&json!({"kind":"shadow","label":label,"candidate_class":candidate_class,"context":"continuity"})),
                Some("extract"),
                Some("skill-trial-run-envelope-v1"),
            )
            .await
            .expect("shadow run");
        record_skill_eval(
            client,
            skill.skill_card_id,
            "promote_shadow",
            "manual_eval",
            true,
            true,
            true,
            0.1,
            Some("promote to shadow"),
            Some("manual_eval"),
            Some(&json!([format!(
                "event:stage3a-negative:eval-shadow:{label}:{suffix}"
            )])),
            Some(&json!([format!(
                "artifact://stage3a-negative/eval-shadow/{label}/{suffix}"
            )])),
            Some(&message_refs),
            Some(&json!({"kind":"eval","phase":"shadow","candidate_class":candidate_class})),
            Some("extract"),
            Some("skill-eval-envelope-v1"),
        )
        .await
        .expect("promote shadow");
        record_skill_trial_run(
                client,
                skill.skill_card_id,
                "trial",
                Some("manual trial"),
                Some("codex"),
                Some("gpt-5"),
                Some("exec_command"),
                true,
                true,
                "success",
                Some("trial success"),
                Some("manual_trial"),
                Some(&json!([format!("event:stage3a-negative:trial:{label}:{suffix}")])),
                Some(&json!([format!(
                    "artifact://stage3a-negative/trial/{label}/{suffix}"
                )])),
                Some(&message_refs),
                Some(&json!({"kind":"trial","label":label,"candidate_class":candidate_class,"context":"continuity"})),
                Some("extract"),
                Some("skill-trial-run-envelope-v1"),
            )
            .await
            .expect("trial run");
        record_skill_eval(
            client,
            skill.skill_card_id,
            "promote_trial",
            "manual_eval",
            true,
            true,
            true,
            0.2,
            Some("promote to trial"),
            Some("manual_eval"),
            Some(&json!([format!(
                "event:stage3a-negative:eval-trial:{label}:{suffix}"
            )])),
            Some(&json!([format!(
                "artifact://stage3a-negative/eval-trial/{label}/{suffix}"
            )])),
            Some(&message_refs),
            Some(&json!({"kind":"eval","phase":"trial","candidate_class":candidate_class})),
            Some("extract"),
            Some("skill-eval-envelope-v1"),
        )
        .await
        .expect("promote trial");
        record_skill_eval(
            client,
            skill.skill_card_id,
            "promote_verified",
            "manual_eval",
            true,
            true,
            true,
            0.3,
            Some("promote to verified"),
            Some("manual_eval"),
            Some(&json!([format!(
                "event:stage3a-negative:eval-verified:{label}:{suffix}"
            )])),
            Some(&json!([format!(
                "artifact://stage3a-negative/eval-verified/{label}/{suffix}"
            )])),
            Some(&message_refs),
            Some(&json!({"kind":"eval","phase":"verified","candidate_class":candidate_class})),
            Some("extract"),
            Some("skill-eval-envelope-v1"),
        )
        .await
        .expect("promote verified");
        list_skill_cards(
            client,
            Some("project_alpha"),
            Some("review"),
            Some(&skill.skill_id),
        )
        .await
        .expect("list skill cards")
        .into_iter()
        .find(|card| card.skill_card_id == skill.skill_card_id)
        .expect("reloaded skill card")
    }

    let success = promote_verified_skill(&client, suffix, "success", "skill_hint").await;
    let failure_pattern =
        promote_verified_skill(&client, suffix, "failure_pattern", "failure_pattern").await;
    let failure_playbook =
        promote_verified_skill(&client, suffix, "failure_playbook", "failure_playbook").await;
    let repair_sequence =
        promote_verified_skill(&client, suffix, "repair_sequence", "repair_sequence").await;
    let anti_pattern =
        promote_verified_skill(&client, suffix, "anti_pattern", "anti_pattern").await;

    let cards = build_skill_execution_cards(
        &client,
        "project_alpha",
        "review",
        Some("continuity"),
        Some("codex"),
        Some("gpt-5"),
        Some("exec_command"),
        false,
        false,
        false,
    )
    .await
    .expect("execution cards");
    let cards = cards.as_array().expect("array");
    assert!(cards.iter().any(|card| {
        card["skill_card_id"] == json!(success.skill_card_id)
            && card["skill_candidate_class"] == json!("skill_hint")
    }));
    assert!(cards.iter().any(|card| {
        card["skill_card_id"] == json!(failure_pattern.skill_card_id)
            && card["skill_candidate_class"] == json!("failure_pattern")
    }));
    assert!(cards.iter().any(|card| {
        card["skill_card_id"] == json!(failure_playbook.skill_card_id)
            && card["skill_candidate_class"] == json!("failure_playbook")
    }));
    assert!(cards.iter().any(|card| {
        card["skill_card_id"] == json!(repair_sequence.skill_card_id)
            && card["skill_candidate_class"] == json!("repair_sequence")
    }));
    assert!(cards.iter().any(|card| {
        card["skill_card_id"] == json!(anti_pattern.skill_card_id)
            && card["skill_candidate_class"] == json!("anti_pattern")
    }));

    let anti_pattern_review =
        super::build_skill_review_payload(&client, anti_pattern.skill_card_id)
            .await
            .expect("anti-pattern review");
    assert_eq!(
        anti_pattern_review["skill"]["skill_candidate_class"],
        json!("anti_pattern")
    );
    assert_eq!(
        anti_pattern_review["skill"]["skill_trust_state"],
        json!("verified")
    );
    assert_eq!(
        anti_pattern_review["evals"]
            .as_array()
            .expect("eval array")
            .iter()
            .map(|value| value["verdict"].as_str().unwrap_or_default())
            .collect::<Vec<_>>(),
        vec!["promote_shadow", "promote_trial", "promote_verified"]
    );
}

#[tokio::test]
async fn project_shared_verified_skill_requires_explicit_shared_approval_for_execution_card() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let Some((key, value)) = trimmed.split_once('=') else {
                continue;
            };
            if std::env::var_os(key).is_none() {
                unsafe {
                    std::env::set_var(key, value);
                }
            }
        }
    }
    unsafe {
        std::env::set_var("AMI_STACK_NAME", "default");
    }
    let cfg = AppConfig::from_env().expect("env config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("unix epoch")
        .as_nanos();
    let namespace_code = format!("review-stage3-shared-approval-{suffix}");
    ensure_project_alpha_test_namespace(&client, &namespace_code).await;

    async fn promote_verified_skill_with_scope(
        client: &Client,
        namespace_code: &str,
        suffix: u128,
        label: &str,
        skill_scope_type: &str,
    ) -> SkillCardRecord {
        let skill = super::create_skill_card_candidate_with_refinement(
            client,
            "project_alpha",
            namespace_code,
            &format!("stage3a_shared_approval_{label}_{suffix}"),
            1,
            &format!("Stage3A shared approval {label}"),
            "Surface verified procedural memory",
            &[format!("trigger {label}")],
            &["continuity fresh".to_string()],
            &[format!("step {label}")],
            &[format!("stop {label}")],
            &[format!("forbidden {label}")],
            Some("proof"),
            skill_scope_type,
            "project",
            &["codex".to_string()],
            &["gpt-5".to_string()],
            &["exec_command".to_string()],
            &["continuity".to_string()],
            &[format!("event:stage3a-shared:{label}:{suffix}")],
            &[format!("artifact://stage3a-shared/{label}/{suffix}")],
            &json!({"path":"docs/AMAI_GLOBAL_MEMORY_ROADMAP.md","line_start":1233,"line_end":1235}),
            Some("skill_hint"),
            Some("new"),
            None,
            None,
            Some("extract"),
        )
        .await
        .expect("skill candidate");

        let message_refs = json!([format!("thread:stage3a-shared:{label}:{suffix}")]);
        create_skill_evidence_bundle(
            client,
            skill.skill_card_id,
            "trace",
            Some("manual evidence"),
            &[format!("event:stage3a-shared:evidence:{label}:{suffix}")],
            &[format!(
                "artifact://stage3a-shared/evidence/{label}/{suffix}"
            )],
            Some("manual_proof"),
            Some(&message_refs),
            Some(&json!({"kind":"bundle","label":label})),
            Some("extract"),
            Some("skill-evidence-bundle-envelope-v1"),
        )
        .await
        .expect("evidence");
        record_skill_trigger_match(
            client,
            skill.skill_card_id,
            "project_task",
            &format!("trigger {label}"),
            true,
            Some("trigger matched"),
            Some("manual_trigger"),
            Some(&json!([format!(
                "event:stage3a-shared:trigger:{label}:{suffix}"
            )])),
            Some(&json!([format!(
                "artifact://stage3a-shared/trigger/{label}/{suffix}"
            )])),
            Some(&message_refs),
            Some(&json!({"kind":"trigger","label":label,"context":"continuity"})),
            Some("extract"),
            Some("skill-trigger-match-envelope-v1"),
        )
        .await
        .expect("trigger");
        record_skill_trial_run(
            client,
            skill.skill_card_id,
            "shadow",
            Some("manual shadow"),
            Some("codex"),
            Some("gpt-5"),
            Some("exec_command"),
            true,
            false,
            "success",
            Some("shadow success"),
            Some("manual_shadow"),
            Some(&json!([format!(
                "event:stage3a-shared:shadow:{label}:{suffix}"
            )])),
            Some(&json!([format!(
                "artifact://stage3a-shared/shadow/{label}/{suffix}"
            )])),
            Some(&message_refs),
            Some(&json!({"kind":"shadow","label":label,"context":"continuity"})),
            Some("extract"),
            Some("skill-trial-run-envelope-v1"),
        )
        .await
        .expect("shadow run");
        record_skill_eval(
            client,
            skill.skill_card_id,
            "promote_shadow",
            "manual_eval",
            true,
            true,
            true,
            0.1,
            Some("promote to shadow"),
            Some("manual_eval"),
            Some(&json!([format!(
                "event:stage3a-shared:eval-shadow:{label}:{suffix}"
            )])),
            Some(&json!([format!(
                "artifact://stage3a-shared/eval-shadow/{label}/{suffix}"
            )])),
            Some(&message_refs),
            Some(&json!({"kind":"eval","phase":"shadow"})),
            Some("extract"),
            Some("skill-eval-envelope-v1"),
        )
        .await
        .expect("promote shadow");
        record_skill_trial_run(
            client,
            skill.skill_card_id,
            "trial",
            Some("manual trial"),
            Some("codex"),
            Some("gpt-5"),
            Some("exec_command"),
            true,
            true,
            "success",
            Some("trial success"),
            Some("manual_trial"),
            Some(&json!([format!(
                "event:stage3a-shared:trial:{label}:{suffix}"
            )])),
            Some(&json!([format!(
                "artifact://stage3a-shared/trial/{label}/{suffix}"
            )])),
            Some(&message_refs),
            Some(&json!({"kind":"trial","label":label,"context":"continuity"})),
            Some("extract"),
            Some("skill-trial-run-envelope-v1"),
        )
        .await
        .expect("trial run");
        record_skill_eval(
            client,
            skill.skill_card_id,
            "promote_trial",
            "manual_eval",
            true,
            true,
            true,
            0.2,
            Some("promote to trial"),
            Some("manual_eval"),
            Some(&json!([format!(
                "event:stage3a-shared:eval-trial:{label}:{suffix}"
            )])),
            Some(&json!([format!(
                "artifact://stage3a-shared/eval-trial/{label}/{suffix}"
            )])),
            Some(&message_refs),
            Some(&json!({"kind":"eval","phase":"trial"})),
            Some("extract"),
            Some("skill-eval-envelope-v1"),
        )
        .await
        .expect("promote trial");
        record_skill_eval(
            client,
            skill.skill_card_id,
            "promote_verified",
            "manual_eval",
            true,
            true,
            true,
            0.3,
            Some("promote to verified"),
            Some("manual_eval"),
            Some(&json!([format!(
                "event:stage3a-shared:eval-verified:{label}:{suffix}"
            )])),
            Some(&json!([format!(
                "artifact://stage3a-shared/eval-verified/{label}/{suffix}"
            )])),
            Some(&message_refs),
            Some(&json!({"kind":"eval","phase":"verified"})),
            Some("extract"),
            Some("skill-eval-envelope-v1"),
        )
        .await
        .expect("promote verified");

        list_skill_cards(
            client,
            Some("project_alpha"),
            Some(namespace_code),
            Some(&skill.skill_id),
        )
        .await
        .expect("list skill cards")
        .into_iter()
        .find(|card| card.skill_card_id == skill.skill_card_id)
        .expect("reloaded skill card")
    }

    let shared = promote_verified_skill_with_scope(
        &client,
        &namespace_code,
        suffix,
        "shared",
        "project_shared",
    )
    .await;
    assert_eq!(shared.skill_trust_state, "verified");
    assert_eq!(shared.skill_shared_promotion_state, "pending_approval");

    let cards_without_approval = build_skill_execution_cards(
        &client,
        "project_alpha",
        &namespace_code,
        Some("continuity"),
        Some("codex"),
        Some("gpt-5"),
        Some("exec_command"),
        false,
        false,
        false,
    )
    .await
    .expect("execution cards without approval");
    assert!(
        !cards_without_approval
            .as_array()
            .expect("array")
            .iter()
            .any(|card| card["skill_card_id"] == json!(shared.skill_card_id))
    );

    let shared_review_before = super::build_skill_review_payload(&client, shared.skill_card_id)
        .await
        .expect("shared review before approval");
    assert_eq!(
        shared_review_before["skill"]["skill_shared_promotion_state"],
        json!("pending_approval")
    );

    record_skill_eval(
        &client,
        shared.skill_card_id,
        "approve_shared_promotion",
        "shared_approval_contour",
        true,
        true,
        true,
        0.0,
        Some("shared procedural approval granted"),
        Some("manual_eval"),
        Some(&json!([format!("event:stage3a-shared:approve:{suffix}")])),
        Some(&json!([format!(
            "artifact://stage3a-shared/approve/{suffix}"
        )])),
        Some(&json!([format!("thread:stage3a-shared:shared:{suffix}")])),
        Some(&json!({"kind":"eval","phase":"shared-approval"})),
        Some("extract"),
        Some("skill-eval-envelope-v1"),
    )
    .await
    .expect("approve shared promotion");

    let cards_with_approval = build_skill_execution_cards(
        &client,
        "project_alpha",
        &namespace_code,
        Some("continuity"),
        Some("codex"),
        Some("gpt-5"),
        Some("exec_command"),
        false,
        false,
        false,
    )
    .await
    .expect("execution cards with approval");
    assert!(
        cards_with_approval
            .as_array()
            .expect("array")
            .iter()
            .any(|card| {
                card["skill_card_id"] == json!(shared.skill_card_id)
                    && card["skill_shared_promotion_state"] == json!("approved")
                    && card["skill_shared_approved_by"] == json!("shared_approval_contour")
            })
    );

    let shared_review_after = super::build_skill_review_payload(&client, shared.skill_card_id)
        .await
        .expect("shared review after approval");
    assert_eq!(
        shared_review_after["skill"]["skill_shared_promotion_state"],
        json!("approved")
    );
    assert_eq!(
        shared_review_after["skill"]["skill_shared_approved_by"],
        json!("shared_approval_contour")
    );
    assert_eq!(
        shared_review_after["skill"]["skill_shared_approval_reason"],
        json!("shared procedural approval granted")
    );
    assert_eq!(
        shared_review_after["evals"]
            .as_array()
            .expect("eval array")
            .iter()
            .map(|value| value["verdict"].as_str().unwrap_or_default())
            .collect::<Vec<_>>(),
        vec![
            "promote_shadow",
            "promote_trial",
            "promote_verified",
            "approve_shared_promotion"
        ]
    );

    let private = promote_verified_skill_with_scope(
        &client,
        &namespace_code,
        suffix,
        "private",
        "project_private",
    )
    .await;
    let cards_private = build_skill_execution_cards(
        &client,
        "project_alpha",
        &namespace_code,
        Some("continuity"),
        Some("codex"),
        Some("gpt-5"),
        Some("exec_command"),
        false,
        false,
        false,
    )
    .await
    .expect("execution cards private");
    assert!(cards_private.as_array().expect("array").iter().any(|card| {
        card["skill_card_id"] == json!(private.skill_card_id)
            && card["skill_shared_promotion_state"] == json!("not_applicable")
    }));
}

#[tokio::test]
async fn build_skill_execution_cards_returns_empty_array_for_without_amai_measurement_mode() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let Some((key, value)) = trimmed.split_once('=') else {
                continue;
            };
            if std::env::var_os(key).is_none() {
                unsafe {
                    std::env::set_var(key, value);
                }
            }
        }
    }
    unsafe {
        std::env::set_var("AMI_STACK_NAME", "default");
    }
    let cfg = AppConfig::from_env().expect("env config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("unix epoch")
        .as_nanos();
    let namespace_code = format!("review-stage3-execution-card-without-amai-{suffix}");
    ensure_project_alpha_test_namespace(&client, &namespace_code).await;

    let skill = super::create_skill_card_candidate_with_refinement(
        &client,
        "project_alpha",
        &namespace_code,
        &format!("stage3a_without_amai_execution_card_{suffix}"),
        1,
        "Stage3A without Amai benchmark control",
        "Surface a verified execution card unless procedural help is bypassed",
        &["continuity restore required".to_string()],
        &["continuity state is fresh".to_string()],
        &["read the execution card".to_string()],
        &["restore task is resolved".to_string()],
        &["continuity state is stale".to_string()],
        Some("verified skill is visible only when Amai may help"),
        "project_private",
        "project",
        &["codex".to_string()],
        &["gpt-5".to_string()],
        &["exec_command".to_string()],
        &["continuity".to_string()],
        &[format!("event:stage3a:without-amai:{suffix}")],
        &[format!("artifact://stage3a/without-amai/{suffix}")],
        &json!({"path":"docs/AMAI_COMPARE_EXPERIMENT_PLAN.md","line_start":76,"line_end":88}),
        Some("skill_hint"),
        Some("new"),
        None,
        None,
        Some("extract"),
    )
    .await
    .expect("skill candidate");
    let message_refs = json!([format!("thread:stage3a:without-amai:{suffix}")]);
    create_skill_evidence_bundle(
        &client,
        skill.skill_card_id,
        "trace",
        Some("manual evidence"),
        &[format!("event:stage3a:without-amai:evidence:{suffix}")],
        &[format!("artifact://stage3a/without-amai/evidence/{suffix}")],
        Some("manual_proof"),
        Some(&message_refs),
        Some(&json!({"kind":"bundle","label":"without-amai"})),
        Some("extract"),
        Some("skill-evidence-bundle-envelope-v1"),
    )
    .await
    .expect("evidence");
    record_skill_trigger_match(
        &client,
        skill.skill_card_id,
        "project_task",
        "continuity restore required",
        true,
        Some("trigger matched"),
        Some("manual_trigger"),
        Some(&json!([format!(
            "event:stage3a:without-amai:trigger:{suffix}"
        )])),
        Some(&json!([format!(
            "artifact://stage3a/without-amai/trigger/{suffix}"
        )])),
        Some(&message_refs),
        Some(&json!({"kind":"trigger","context":"continuity"})),
        Some("extract"),
        Some("skill-trigger-match-envelope-v1"),
    )
    .await
    .expect("trigger");
    record_skill_trial_run(
        &client,
        skill.skill_card_id,
        "shadow",
        Some("manual shadow"),
        Some("codex"),
        Some("gpt-5"),
        Some("exec_command"),
        true,
        false,
        "success",
        Some("shadow success"),
        Some("manual_shadow"),
        Some(&json!([format!(
            "event:stage3a:without-amai:shadow:{suffix}"
        )])),
        Some(&json!([format!(
            "artifact://stage3a/without-amai/shadow/{suffix}"
        )])),
        Some(&message_refs),
        Some(&json!({"kind":"shadow","context":"continuity"})),
        Some("extract"),
        Some("skill-trial-run-envelope-v1"),
    )
    .await
    .expect("shadow run");
    record_skill_eval(
        &client,
        skill.skill_card_id,
        "promote_shadow",
        "manual_eval",
        true,
        true,
        true,
        0.1,
        Some("promote to shadow"),
        Some("manual_eval"),
        Some(&json!([format!(
            "event:stage3a:without-amai:eval-shadow:{suffix}"
        )])),
        Some(&json!([format!(
            "artifact://stage3a/without-amai/eval-shadow/{suffix}"
        )])),
        Some(&message_refs),
        Some(&json!({"kind":"eval","phase":"shadow"})),
        Some("extract"),
        Some("skill-eval-envelope-v1"),
    )
    .await
    .expect("promote shadow");
    record_skill_trial_run(
        &client,
        skill.skill_card_id,
        "trial",
        Some("manual trial"),
        Some("codex"),
        Some("gpt-5"),
        Some("exec_command"),
        true,
        true,
        "success",
        Some("trial success"),
        Some("manual_trial"),
        Some(&json!([format!(
            "event:stage3a:without-amai:trial:{suffix}"
        )])),
        Some(&json!([format!(
            "artifact://stage3a/without-amai/trial/{suffix}"
        )])),
        Some(&message_refs),
        Some(&json!({"kind":"trial","context":"continuity"})),
        Some("extract"),
        Some("skill-trial-run-envelope-v1"),
    )
    .await
    .expect("trial run");
    record_skill_eval(
        &client,
        skill.skill_card_id,
        "promote_trial",
        "manual_eval",
        true,
        true,
        true,
        0.2,
        Some("promote to trial"),
        Some("manual_eval"),
        Some(&json!([format!(
            "event:stage3a:without-amai:eval-trial:{suffix}"
        )])),
        Some(&json!([format!(
            "artifact://stage3a/without-amai/eval-trial/{suffix}"
        )])),
        Some(&message_refs),
        Some(&json!({"kind":"eval","phase":"trial"})),
        Some("extract"),
        Some("skill-eval-envelope-v1"),
    )
    .await
    .expect("promote trial");
    record_skill_eval(
        &client,
        skill.skill_card_id,
        "promote_verified",
        "manual_eval",
        true,
        true,
        true,
        0.3,
        Some("promote to verified"),
        Some("manual_eval"),
        Some(&json!([format!(
            "event:stage3a:without-amai:eval-verified:{suffix}"
        )])),
        Some(&json!([format!(
            "artifact://stage3a/without-amai/eval-verified/{suffix}"
        )])),
        Some(&message_refs),
        Some(&json!({"kind":"eval","phase":"verified"})),
        Some("extract"),
        Some("skill-eval-envelope-v1"),
    )
    .await
    .expect("promote verified");

    let with_amai_cards = build_skill_execution_cards(
        &client,
        "project_alpha",
        &namespace_code,
        Some("continuity"),
        Some("codex"),
        Some("gpt-5"),
        Some("exec_command"),
        true,
        false,
        false,
    )
    .await
    .expect("with amai execution cards");
    assert_eq!(with_amai_cards.as_array().expect("array").len(), 1);

    let without_amai_cards = build_skill_execution_cards(
        &client,
        "project_alpha",
        &namespace_code,
        Some("continuity"),
        Some("codex"),
        Some("gpt-5"),
        Some("exec_command"),
        true,
        true,
        true,
    )
    .await
    .expect("without amai execution cards");
    assert!(without_amai_cards.as_array().expect("array").is_empty());
}

#[tokio::test]
async fn create_task_node_surfaces_stage2_runtime_fields() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            };
            let Some((key, value)) = trimmed.split_once('=') else {
                continue;
            };
            if std::env::var_os(key).is_none() {
                unsafe {
                    std::env::set_var(key, value);
                }
            }
        }
    }
    unsafe {
        std::env::set_var("AMI_STACK_NAME", "default");
    }
    let cfg = AppConfig::from_env().expect("env config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("unix epoch")
        .as_nanos();
    let source_event_ids = json!([format!("event:task-node:{suffix}")]);
    let artifact_refs = json!([format!("artifact://proof/task-node/{suffix}")]);
    let evidence_span = json!({
        "event_id": format!("event:task-node:{suffix}"),
        "snapshot_id": format!("snapshot:task-node:{suffix}")
    });
    let status_payload = json!({
        "source_kind": "continuity_handoff",
        "source_event_id": format!("event:task-node:{suffix}"),
        "source_snapshot_id": format!("snapshot:task-node:{suffix}")
    });
    let task_key = format!("task-node-{suffix}");
    let metadata = json!({
        "local_path": format!("/tmp/task-node-{suffix}.md"),
        "materialized_from": "proof"
    });

    let task_node = create_task_node(
        &client,
        "project_alpha",
        "review",
        &TaskNodeInsert {
            parent_task_node_id: None,
            memory_item_id: None,
            task_key: Some(&task_key),
            task_role: Some("proposal"),
            headline: "Decision: reopen continuity workline",
            summary: Some("summary"),
            next_step: Some("next step"),
            execution_state: Some("active"),
            lifecycle_state: Some("hot"),
            confidence: Some(1.0),
            current_score: None,
            reopened_count: Some(0),
            child_count: Some(0),
            closed_child_count: Some(0),
            pending_return_count: Some(0),
            source_event_ids: Some(&source_event_ids),
            artifact_refs: Some(&artifact_refs),
            evidence_span: Some(&evidence_span),
            derivation_kind: Some("extract"),
            status_payload: &status_payload,
            metadata: &metadata,
            opened_at_epoch_ms: Some(1000),
            closed_at_epoch_ms: None,
            archived_at_epoch_ms: None,
        },
    )
    .await
    .expect("task node");

    assert_eq!(task_node.source_event_ids, source_event_ids);
    assert_eq!(task_node.artifact_refs, artifact_refs);
    assert_eq!(task_node.evidence_span, evidence_span);
    assert_eq!(task_node.candidate_class, "commitment");
    assert_eq!(task_node.derivation_kind, "extract");
    assert_eq!(task_node.source_kind.as_deref(), Some("continuity_handoff"));
    assert!(task_node.hot_path_write_eligible);
    assert!(!task_node.background_consolidation_recommended);
    assert_eq!(
        task_node.metadata["stage2_runtime"]["candidate_class"].as_str(),
        Some("commitment")
    );
    assert_eq!(
        task_node.metadata["stage2_runtime"]["policy_and_scope_filter"]["visibility_scope"],
        json!("project_shared")
    );
    assert_eq!(
        task_node.metadata["stage2_runtime"]["verification_conflict_check"]["duplicate_task_key_conflict"],
        json!(false)
    );
}

#[tokio::test]
async fn create_task_node_rejects_duplicate_task_key() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            };
            let Some((key, value)) = trimmed.split_once('=') else {
                continue;
            };
            if std::env::var_os(key).is_none() {
                unsafe {
                    std::env::set_var(key, value);
                }
            }
        }
    }
    unsafe {
        std::env::set_var("AMI_STACK_NAME", "default");
    }
    let cfg = AppConfig::from_env().expect("env config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("unix epoch")
        .as_nanos();
    let task_key = format!("task-node-duplicate-{suffix}");

    create_task_node(
        &client,
        "project_alpha",
        "review",
        &TaskNodeInsert {
            parent_task_node_id: None,
            memory_item_id: None,
            task_key: Some(&task_key),
            task_role: Some("proposal"),
            headline: "Decision: reopen continuity workline",
            summary: Some("summary"),
            next_step: Some("next step"),
            execution_state: Some("active"),
            lifecycle_state: Some("hot"),
            confidence: Some(1.0),
            current_score: None,
            reopened_count: Some(0),
            child_count: Some(0),
            closed_child_count: Some(0),
            pending_return_count: Some(0),
            source_event_ids: Some(&json!([format!("event:task-node:first:{suffix}")])),
            artifact_refs: Some(&json!([format!(
                "artifact://proof/task-node/first/{suffix}"
            )])),
            evidence_span: Some(&json!({
                "event_id": format!("event:task-node:first:{suffix}"),
                "snapshot_id": format!("snapshot:task-node:first:{suffix}")
            })),
            derivation_kind: Some("extract"),
            status_payload: &json!({
                "source_kind": "continuity_handoff",
                "source_event_id": format!("event:task-node:first:{suffix}"),
                "source_snapshot_id": format!("snapshot:task-node:first:{suffix}")
            }),
            metadata: &json!({
                "local_path": format!("/tmp/task-node-first-{suffix}.md"),
                "materialized_from": "proof"
            }),
            opened_at_epoch_ms: Some(1000),
            closed_at_epoch_ms: None,
            archived_at_epoch_ms: None,
        },
    )
    .await
    .expect("first task node");

    let error = create_task_node(
        &client,
        "project_alpha",
        "review",
        &TaskNodeInsert {
            parent_task_node_id: None,
            memory_item_id: None,
            task_key: Some(&task_key),
            task_role: Some("proposal"),
            headline: "Decision: reopen continuity workline",
            summary: Some("summary"),
            next_step: Some("next step"),
            execution_state: Some("active"),
            lifecycle_state: Some("hot"),
            confidence: Some(1.0),
            current_score: None,
            reopened_count: Some(0),
            child_count: Some(0),
            closed_child_count: Some(0),
            pending_return_count: Some(0),
            source_event_ids: Some(&json!([format!("event:task-node:second:{suffix}")])),
            artifact_refs: Some(&json!([format!(
                "artifact://proof/task-node/second/{suffix}"
            )])),
            evidence_span: Some(&json!({
                "event_id": format!("event:task-node:second:{suffix}"),
                "snapshot_id": format!("snapshot:task-node:second:{suffix}")
            })),
            derivation_kind: Some("extract"),
            status_payload: &json!({
                "source_kind": "continuity_handoff",
                "source_event_id": format!("event:task-node:second:{suffix}"),
                "source_snapshot_id": format!("snapshot:task-node:second:{suffix}")
            }),
            metadata: &json!({
                "local_path": format!("/tmp/task-node-second-{suffix}.md"),
                "materialized_from": "proof"
            }),
            opened_at_epoch_ms: Some(1001),
            closed_at_epoch_ms: None,
            archived_at_epoch_ms: None,
        },
    )
    .await
    .expect_err("duplicate task key rejected");
    assert!(
        error
            .to_string()
            .contains("existing task_key in the same namespace")
    );
}

#[tokio::test]
async fn create_task_node_rejects_duplicate_task_key_even_when_existing_line_is_closed() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            };
            let Some((key, value)) = trimmed.split_once('=') else {
                continue;
            };
            if std::env::var_os(key).is_none() {
                unsafe {
                    std::env::set_var(key, value);
                }
            }
        }
    }
    unsafe {
        std::env::set_var("AMI_STACK_NAME", "default");
    }
    let cfg = AppConfig::from_env().expect("env config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("unix epoch")
        .as_nanos();
    let task_key = format!("task-node-duplicate-closed-{suffix}");

    create_task_node(
        &client,
        "project_alpha",
        "review",
        &TaskNodeInsert {
            parent_task_node_id: None,
            memory_item_id: None,
            task_key: Some(&task_key),
            task_role: Some("workline"),
            headline: "Closed workline",
            summary: Some("summary"),
            next_step: Some("none"),
            execution_state: Some("done"),
            lifecycle_state: Some("closed"),
            confidence: Some(1.0),
            current_score: None,
            reopened_count: Some(0),
            child_count: Some(0),
            closed_child_count: Some(0),
            pending_return_count: Some(0),
            source_event_ids: Some(&json!([format!("event:task-node:closed:{suffix}")])),
            artifact_refs: Some(&json!([format!(
                "artifact://proof/task-node/closed/{suffix}"
            )])),
            evidence_span: Some(&json!({
                "event_id": format!("event:task-node:closed:{suffix}"),
                "snapshot_id": format!("snapshot:task-node:closed:{suffix}")
            })),
            derivation_kind: Some("extract"),
            status_payload: &json!({
                "source_kind": "continuity_handoff",
                "source_event_id": format!("event:task-node:closed:{suffix}"),
                "source_snapshot_id": format!("snapshot:task-node:closed:{suffix}")
            }),
            metadata: &json!({
                "local_path": format!("/tmp/task-node-closed-{suffix}.md"),
                "materialized_from": "proof"
            }),
            opened_at_epoch_ms: Some(1000),
            closed_at_epoch_ms: Some(1001),
            archived_at_epoch_ms: None,
        },
    )
    .await
    .expect("first closed task node");

    let error = create_task_node(
        &client,
        "project_alpha",
        "review",
        &TaskNodeInsert {
            parent_task_node_id: None,
            memory_item_id: None,
            task_key: Some(&task_key),
            task_role: Some("proposal"),
            headline: "Duplicate closed workline",
            summary: Some("summary"),
            next_step: Some("resume instead"),
            execution_state: Some("proposed"),
            lifecycle_state: Some("hot"),
            confidence: Some(1.0),
            current_score: None,
            reopened_count: Some(0),
            child_count: Some(0),
            closed_child_count: Some(0),
            pending_return_count: Some(0),
            source_event_ids: Some(&json!([format!(
                "event:task-node:duplicate-closed:{suffix}"
            )])),
            artifact_refs: Some(&json!([format!(
                "artifact://proof/task-node/duplicate-closed/{suffix}"
            )])),
            evidence_span: Some(&json!({
                "event_id": format!("event:task-node:duplicate-closed:{suffix}"),
                "snapshot_id": format!("snapshot:task-node:duplicate-closed:{suffix}")
            })),
            derivation_kind: Some("extract"),
            status_payload: &json!({
                "source_kind": "continuity_handoff",
                "source_event_id": format!("event:task-node:duplicate-closed:{suffix}"),
                "source_snapshot_id": format!("snapshot:task-node:duplicate-closed:{suffix}")
            }),
            metadata: &json!({
                "local_path": format!("/tmp/task-node-duplicate-closed-{suffix}.md"),
                "materialized_from": "proof"
            }),
            opened_at_epoch_ms: Some(1002),
            closed_at_epoch_ms: None,
            archived_at_epoch_ms: None,
        },
    )
    .await
    .expect_err("duplicate closed task key rejected");
    assert!(
        error
            .to_string()
            .contains("existing task_key in the same namespace")
    );
}

#[tokio::test]
async fn create_task_event_surfaces_raw_event_provenance_fields() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            };
            let Some((key, value)) = trimmed.split_once('=') else {
                continue;
            };
            if std::env::var_os(key).is_none() {
                unsafe {
                    std::env::set_var(key, value);
                }
            }
        }
    }
    unsafe {
        std::env::set_var("AMI_STACK_NAME", "default");
    }
    let cfg = AppConfig::from_env().expect("env config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("unix epoch")
        .as_nanos();
    let source_event_id = format!("event:task-event:{suffix}");
    let task_key = format!("task-node-for-event-{suffix}");
    let task_node = create_task_node(
            &client,
            "project_alpha",
            "review",
            &TaskNodeInsert {
                parent_task_node_id: None,
                memory_item_id: None,
                task_key: Some(&task_key),
                task_role: Some("workline"),
                headline: "Task node for event proof",
                summary: Some("summary"),
                next_step: Some("next"),
                execution_state: Some("active"),
                lifecycle_state: Some("hot"),
                confidence: Some(1.0),
                current_score: None,
                reopened_count: Some(0),
                child_count: Some(0),
                closed_child_count: Some(0),
                pending_return_count: Some(0),
                source_event_ids: Some(&json!([source_event_id.clone()])),
                artifact_refs: Some(&json!([format!("artifact://proof/task-event/node/{suffix}")])),
                evidence_span: Some(&json!({"event_id":source_event_id,"kind":"task_node"})),
                derivation_kind: Some("extract"),
                status_payload: &json!({"source_kind":"continuity_handoff","source_event_id":source_event_id}),
                metadata: &json!({"local_path":format!("/tmp/task-event-node-{suffix}.md")}),
                opened_at_epoch_ms: Some(1000),
                closed_at_epoch_ms: None,
                archived_at_epoch_ms: None,
            },
        )
        .await
        .expect("task node");
    let artifact_refs = json!([format!("artifact://proof/task-event/{suffix}")]);
    let message_refs = json!([format!("thread:{suffix}")]);
    let evidence_span = json!({
        "event_id": source_event_id,
        "snapshot_id": format!("snapshot:task-event:{suffix}"),
        "kind": "continuity_handoff",
    });
    let payload = json!({"source_kind":"continuity_handoff","summary":"summary"});
    let event = create_task_event(
        &client,
        "project_alpha",
        "review",
        &TaskEventInsert {
            task_node_id: task_node.task_node_id,
            source_snapshot_id: None,
            source_event_id: Some(&source_event_id),
            event_kind: "created",
            prior_execution_state: None,
            next_execution_state: Some("active"),
            prior_lifecycle_state: None,
            next_lifecycle_state: Some("hot"),
            source_kind: Some("continuity_handoff"),
            artifact_refs: Some(&artifact_refs),
            message_refs: Some(&message_refs),
            evidence_span: Some(&evidence_span),
            derivation_kind: Some("raw_capture"),
            schema_version: Some("task-event-envelope-v1"),
            event_payload: &payload,
            recorded_at_epoch_ms: Some(1001),
        },
    )
    .await
    .expect("task event");
    assert_eq!(event.source_kind.as_deref(), Some("continuity_handoff"));
    assert_eq!(event.artifact_refs, artifact_refs);
    assert_eq!(event.message_refs, message_refs);
    assert_eq!(event.evidence_span, evidence_span);
    assert_eq!(event.derivation_kind, "raw_capture");
    assert_eq!(event.schema_version, "task-event-envelope-v1");
}

#[tokio::test]
async fn create_task_event_accepts_state_transition_alias() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            };
            let Some((key, value)) = trimmed.split_once('=') else {
                continue;
            };
            if std::env::var_os(key).is_none() {
                unsafe {
                    std::env::set_var(key, value);
                }
            }
        }
    }
    unsafe {
        std::env::set_var("AMI_STACK_NAME", "default");
    }
    let cfg = AppConfig::from_env().expect("env config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("unix epoch")
        .as_nanos();
    let source_event_id = format!("event:task-event-alias:{suffix}");
    let task_key = format!("task-node-for-event-alias-{suffix}");
    let task_node = create_task_node(
            &client,
            "project_alpha",
            "review",
            &TaskNodeInsert {
                parent_task_node_id: None,
                memory_item_id: None,
                task_key: Some(&task_key),
                task_role: Some("workline"),
                headline: "Task node for event alias proof",
                summary: Some("summary"),
                next_step: Some("next"),
                execution_state: Some("proposed"),
                lifecycle_state: Some("hot"),
                confidence: Some(1.0),
                current_score: None,
                reopened_count: Some(0),
                child_count: Some(0),
                closed_child_count: Some(0),
                pending_return_count: Some(0),
                source_event_ids: Some(&json!([source_event_id.clone()])),
                artifact_refs: Some(&json!([format!(
                    "artifact://proof/task-event-alias/node/{suffix}"
                )])),
                evidence_span: Some(&json!({"event_id":source_event_id,"kind":"task_node"})),
                derivation_kind: Some("extract"),
                status_payload: &json!({"source_kind":"continuity_handoff","source_event_id":source_event_id}),
                metadata: &json!({"local_path":format!("/tmp/task-event-alias-node-{suffix}.md")}),
                opened_at_epoch_ms: Some(1000),
                closed_at_epoch_ms: None,
                archived_at_epoch_ms: None,
            },
        )
        .await
        .expect("task node");
    let event = create_task_event(
            &client,
            "project_alpha",
            "review",
            &TaskEventInsert {
                task_node_id: task_node.task_node_id,
                source_snapshot_id: None,
                source_event_id: Some(&source_event_id),
                event_kind: "state_transition",
                prior_execution_state: Some("proposed"),
                next_execution_state: Some("active"),
                prior_lifecycle_state: Some("hot"),
                next_lifecycle_state: Some("hot"),
                source_kind: Some("continuity_handoff"),
                artifact_refs: Some(&json!([format!("artifact://proof/task-event-alias/{suffix}")])) ,
                message_refs: Some(&json!([format!("message:task-event-alias:{suffix}")])) ,
                evidence_span: Some(&json!({"event_id":source_event_id,"kind":"task_event"})),
                derivation_kind: Some("raw_capture"),
                schema_version: Some("task-event-envelope-v1"),
                event_payload: &json!({"source_kind":"continuity_handoff","transition":"proposal_to_active"}),
                recorded_at_epoch_ms: Some(1002),
            },
        )
        .await
        .expect("task event");
    assert_eq!(event.event_kind, "state_change");
    assert_eq!(event.prior_execution_state.as_deref(), Some("proposed"));
    assert_eq!(event.next_execution_state.as_deref(), Some("active"));
}

#[tokio::test]
async fn create_task_event_materializes_resumed_state_on_task_node() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            };
            let Some((key, value)) = trimmed.split_once('=') else {
                continue;
            };
            if std::env::var_os(key).is_none() {
                unsafe {
                    std::env::set_var(key, value);
                }
            }
        }
    }
    unsafe {
        std::env::set_var("AMI_STACK_NAME", "default");
    }
    let cfg = AppConfig::from_env().expect("env config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("unix epoch")
        .as_nanos();
    let task_key = format!("task-node-resume-{suffix}");
    let task_node = create_task_node(
        &client,
        "project_alpha",
        "review",
        &TaskNodeInsert {
            parent_task_node_id: None,
            memory_item_id: None,
            task_key: Some(&task_key),
            task_role: Some("workline"),
            headline: "Task node for resumed event proof",
            summary: Some("summary"),
            next_step: Some("resume"),
            execution_state: Some("done"),
            lifecycle_state: Some("closed"),
            confidence: Some(1.0),
            current_score: None,
            reopened_count: Some(0),
            child_count: Some(0),
            closed_child_count: Some(0),
            pending_return_count: Some(0),
            source_event_ids: Some(&json!([format!("event:task-node-resume:{suffix}")])),
            artifact_refs: Some(&json!([format!(
                "artifact://proof/task-node-resume/{suffix}"
            )])),
            evidence_span: Some(&json!({"event_id":format!("event:task-node-resume:{suffix}")})),
            derivation_kind: Some("extract"),
            status_payload: &json!({"source_kind":"continuity_handoff"}),
            metadata: &json!({"local_path":format!("/tmp/task-node-resume-{suffix}.md")}),
            opened_at_epoch_ms: Some(1000),
            closed_at_epoch_ms: Some(1001),
            archived_at_epoch_ms: None,
        },
    )
    .await
    .expect("task node");

    create_task_event(
        &client,
        "project_alpha",
        "review",
        &TaskEventInsert {
            task_node_id: task_node.task_node_id,
            source_snapshot_id: None,
            source_event_id: Some(&format!("event:task-node-resume-reopen:{suffix}")),
            event_kind: "resumed",
            prior_execution_state: Some("done"),
            next_execution_state: Some("active"),
            prior_lifecycle_state: Some("closed"),
            next_lifecycle_state: Some("hot"),
            source_kind: Some("continuity_handoff"),
            artifact_refs: Some(&json!([format!(
                "artifact://proof/task-node-resume-reopen/{suffix}"
            )])),
            message_refs: Some(&json!([format!("message:task-node-resume:{suffix}")])),
            evidence_span: Some(
                &json!({"event_id":format!("event:task-node-resume-reopen:{suffix}")}),
            ),
            derivation_kind: Some("raw_capture"),
            schema_version: Some("task-event-envelope-v1"),
            event_payload: &json!({"transition":"resume"}),
            recorded_at_epoch_ms: Some(1002),
        },
    )
    .await
    .expect("resumed event");

    let refreshed = get_task_node(&client, task_node.task_node_id)
        .await
        .expect("refreshed task node");
    assert_eq!(refreshed.execution_state, "active");
    assert_eq!(refreshed.lifecycle_state, "hot");
    assert_eq!(refreshed.closed_at_epoch_ms, None);
    assert_eq!(refreshed.archived_at_epoch_ms, None);
    assert_eq!(refreshed.reopened_count, 1);
}

#[tokio::test]
async fn stage2_system_tables_surface_provenance_columns() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            };
            let Some((key, value)) = trimmed.split_once('=') else {
                continue;
            };
            if std::env::var_os(key).is_none() {
                unsafe {
                    std::env::set_var(key, value);
                }
            }
        }
    }
    unsafe {
        std::env::set_var("AMI_STACK_NAME", "default");
    }
    let cfg = AppConfig::from_env().expect("env config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let count: i64 = client
            .query_one(
                r#"
                SELECT count(*)
                FROM information_schema.columns
                WHERE table_schema = 'ami'
                  AND (
                    (table_name = 'retrieval_traces' AND column_name IN ('source_kind','source_event_ids','artifact_refs','message_refs','evidence_span','derivation_kind','schema_version'))
                    OR (table_name = 'restore_packs' AND column_name IN ('source_kind','source_event_ids','artifact_refs','message_refs','evidence_span','derivation_kind','schema_version'))
                    OR (table_name = 'policy_rules' AND column_name IN ('source_kind','source_event_ids','artifact_refs','message_refs','evidence_span','derivation_kind','schema_version'))
                    OR (table_name = 'quarantine_items' AND column_name IN ('source_kind','source_event_ids','artifact_refs','message_refs','evidence_span','derivation_kind','schema_version'))
                    OR (table_name = 'memory_edges' AND column_name IN ('source_kind','source_event_ids','artifact_refs','message_refs','evidence_span','derivation_kind','schema_version'))
                    OR (table_name = 'memory_conflicts' AND column_name IN ('source_kind','source_event_ids','artifact_refs','message_refs','evidence_span','derivation_kind','schema_version'))
                  )
                "#,
                &[],
            )
            .await
            .expect("column count")
            .get(0);
    assert_eq!(count, 42);
}

#[tokio::test]
async fn create_memory_link_decision_surfaces_stage2_provenance_fields() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            };
            let Some((key, value)) = trimmed.split_once('=') else {
                continue;
            };
            if std::env::var_os(key).is_none() {
                unsafe {
                    std::env::set_var(key, value);
                }
            }
        }
    }
    unsafe {
        std::env::set_var("AMI_STACK_NAME", "default");
    }
    let cfg = AppConfig::from_env().expect("env config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("unix epoch")
        .as_nanos();
    let source_event_id = format!("event:link-decision:{suffix}");
    let task_key = format!("task-node-for-link-decision-{suffix}");
    let task_node = create_task_node(
            &client,
            "project_alpha",
            "review",
            &TaskNodeInsert {
                parent_task_node_id: None,
                memory_item_id: None,
                task_key: Some(&task_key),
                task_role: Some("proposal"),
                headline: "Task node for link decision proof",
                summary: Some("summary"),
                next_step: Some("next"),
                execution_state: Some("active"),
                lifecycle_state: Some("hot"),
                confidence: Some(1.0),
                current_score: None,
                reopened_count: Some(0),
                child_count: Some(0),
                closed_child_count: Some(0),
                pending_return_count: Some(0),
                source_event_ids: Some(&json!([source_event_id.clone()])),
                artifact_refs: Some(&json!([format!(
                    "artifact://proof/link-decision/node/{suffix}"
                )])),
                evidence_span: Some(&json!({"event_id":source_event_id,"kind":"task_node"})),
                derivation_kind: Some("extract"),
                status_payload: &json!({"source_kind":"continuity_handoff","source_event_id":source_event_id}),
                metadata: &json!({"local_path":format!("/tmp/link-decision-node-{suffix}.md")}),
                opened_at_epoch_ms: Some(1000),
                closed_at_epoch_ms: None,
                archived_at_epoch_ms: None,
            },
        )
        .await
        .expect("task node");
    let source_event_ids = json!([format!("event:link-decision:basis:{suffix}")]);
    let artifact_refs = json!([format!("artifact://proof/link-decision/{suffix}")]);
    let message_refs = json!([format!("thread:link-decision:{suffix}")]);
    let evidence_span = json!({
        "event_id": source_event_id,
        "candidate_task_key": task_key,
        "kind": "routing_decision",
    });
    let payload = json!({
        "scope_filtering": "pass",
        "candidate_generation": "shortlist",
        "rerank": "classifier",
        "evidence_sufficiency_check": "pass",
        "routing_decision": "continue",
    });
    let decision = create_memory_link_decision(
        &client,
        "project_alpha",
        "review",
        &MemoryLinkDecisionInsert {
            task_node_id: Some(task_node.task_node_id),
            retrieval_trace_id: None,
            candidate_task_node_id: Some(task_node.task_node_id),
            decision_outcome: "continue",
            legality_passed: true,
            scope_filter_passed: true,
            evidence_sufficient: true,
            classifier_label: Some("continue_existing_branch"),
            classifier_score: Some(0.99),
            decision_reason: Some("strong contour match"),
            decision_payload: &payload,
            source_event_ids: Some(&source_event_ids),
            artifact_refs: Some(&artifact_refs),
            message_refs: Some(&message_refs),
            evidence_span: Some(&evidence_span),
            derivation_kind: Some("extract"),
            schema_version: Some("memory-link-decision-envelope-v1"),
            recorded_at_epoch_ms: Some(1002),
        },
    )
    .await
    .expect("memory link decision");
    assert_eq!(decision.source_event_ids, source_event_ids);
    assert_eq!(decision.artifact_refs, artifact_refs);
    assert_eq!(decision.message_refs, message_refs);
    assert_eq!(decision.evidence_span["kind"], evidence_span["kind"]);
    assert_eq!(
        decision.evidence_span["stage2_runtime"]["policy_and_scope_filter"]["scope_binding_valid"],
        json!(true)
    );
    assert_eq!(
        decision.evidence_span["stage2_runtime"]["verification_conflict_check"]["write_allowed"],
        json!(true)
    );
    assert_eq!(decision.derivation_kind, "extract");
    assert_eq!(decision.schema_version, "memory-link-decision-envelope-v1");
}

#[tokio::test]
async fn create_memory_link_decision_continue_materializes_candidate_resume() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = trimmed.split_once('=') {
                unsafe {
                    std::env::set_var(key.trim(), value.trim_matches('\"'));
                }
            }
        }
    }
    unsafe {
        std::env::set_var("AMI_STACK_NAME", "default");
    }
    let cfg = AppConfig::from_env().expect("env config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("unix epoch")
        .as_nanos();
    let candidate = create_task_node(
        &client,
        "project_alpha",
        "review",
        &TaskNodeInsert {
            parent_task_node_id: None,
            memory_item_id: None,
            task_key: Some(&format!("candidate-resume-{suffix}")),
            task_role: Some("workline"),
            headline: "Closed candidate",
            summary: Some("closed"),
            next_step: Some("resume"),
            execution_state: Some("done"),
            lifecycle_state: Some("closed"),
            confidence: Some(1.0),
            current_score: None,
            reopened_count: Some(0),
            child_count: Some(0),
            closed_child_count: Some(0),
            pending_return_count: Some(0),
            source_event_ids: Some(&json!([format!("event:candidate-resume:{suffix}")])),
            artifact_refs: Some(&json!([format!(
                "artifact://proof/candidate-resume/{suffix}"
            )])),
            evidence_span: Some(&json!({"kind":"task_node"})),
            derivation_kind: Some("operator_write"),
            status_payload: &json!({}),
            metadata: &json!({}),
            opened_at_epoch_ms: None,
            closed_at_epoch_ms: Some(2000),
            archived_at_epoch_ms: None,
        },
    )
    .await
    .expect("candidate");
    let incoming = create_task_node(
        &client,
        "project_alpha",
        "review",
        &TaskNodeInsert {
            parent_task_node_id: None,
            memory_item_id: None,
            task_key: Some(&format!("incoming-resume-{suffix}")),
            task_role: Some("proposal"),
            headline: "Incoming proposal",
            summary: Some("incoming"),
            next_step: Some("route"),
            execution_state: Some("active"),
            lifecycle_state: Some("hot"),
            confidence: Some(1.0),
            current_score: None,
            reopened_count: Some(0),
            child_count: Some(0),
            closed_child_count: Some(0),
            pending_return_count: Some(0),
            source_event_ids: Some(&json!([format!("event:incoming-resume:{suffix}")])),
            artifact_refs: Some(&json!([format!(
                "artifact://proof/incoming-resume/{suffix}"
            )])),
            evidence_span: Some(&json!({"kind":"task_node"})),
            derivation_kind: Some("operator_write"),
            status_payload: &json!({}),
            metadata: &json!({}),
            opened_at_epoch_ms: Some(2500),
            closed_at_epoch_ms: None,
            archived_at_epoch_ms: None,
        },
    )
    .await
    .expect("incoming");
    let decision = create_memory_link_decision(
        &client,
        "project_alpha",
        "review",
        &MemoryLinkDecisionInsert {
            task_node_id: Some(incoming.task_node_id),
            retrieval_trace_id: None,
            candidate_task_node_id: Some(candidate.task_node_id),
            decision_outcome: "continue",
            legality_passed: true,
            scope_filter_passed: true,
            evidence_sufficient: true,
            classifier_label: Some("continue_existing_branch"),
            classifier_score: Some(0.97),
            decision_reason: Some("same branch"),
            decision_payload: &json!({"routing":"continue"}),
            source_event_ids: Some(&json!([format!("event:continue-decision:{suffix}")])),
            artifact_refs: Some(&json!([format!(
                "artifact://proof/continue-decision/{suffix}"
            )])),
            message_refs: Some(&json!([format!(
                "message://proof/continue-decision/{suffix}"
            )])),
            evidence_span: Some(&json!({"kind":"routing_decision"})),
            derivation_kind: Some("extract"),
            schema_version: Some("memory-link-decision-envelope-v1"),
            recorded_at_epoch_ms: Some(3000),
        },
    )
    .await
    .expect("decision");
    let candidate_after = get_task_node(&client, candidate.task_node_id)
        .await
        .expect("candidate after");
    assert_eq!(candidate_after.execution_state, "active");
    assert_eq!(candidate_after.lifecycle_state, "hot");
    assert_eq!(candidate_after.reopened_count, 1);
    assert_eq!(candidate_after.closed_at_epoch_ms, None);
    let continued_events = client
            .query(
                "SELECT event_kind FROM ami.task_events WHERE task_node_id = $1 AND source_event_id = $2",
                &[&candidate.task_node_id, &format!("memory_link_decision:{}", decision.memory_link_decision_id)],
            )
            .await
            .expect("events");
    assert_eq!(continued_events.len(), 1);
    let event_kind: String = continued_events[0].get(0);
    assert_eq!(event_kind, "resumed");
}

#[tokio::test]
async fn create_memory_link_decision_child_materializes_parent_rollups() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = trimmed.split_once('=') {
                unsafe {
                    std::env::set_var(key.trim(), value.trim_matches('\"'));
                }
            }
        }
    }
    unsafe {
        std::env::set_var("AMI_STACK_NAME", "default");
    }
    let cfg = AppConfig::from_env().expect("env config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("unix epoch")
        .as_nanos();
    let parent = create_task_node(
        &client,
        "project_alpha",
        "review",
        &TaskNodeInsert {
            parent_task_node_id: None,
            memory_item_id: None,
            task_key: Some(&format!("parent-child-{suffix}")),
            task_role: Some("workline"),
            headline: "Parent branch",
            summary: Some("parent"),
            next_step: Some("drive"),
            execution_state: Some("active"),
            lifecycle_state: Some("hot"),
            confidence: Some(1.0),
            current_score: None,
            reopened_count: Some(0),
            child_count: Some(0),
            closed_child_count: Some(0),
            pending_return_count: Some(0),
            source_event_ids: Some(&json!([format!("event:parent-child:{suffix}")])),
            artifact_refs: Some(&json!([format!("artifact://proof/parent-child/{suffix}")])),
            evidence_span: Some(&json!({"kind":"task_node"})),
            derivation_kind: Some("operator_write"),
            status_payload: &json!({}),
            metadata: &json!({}),
            opened_at_epoch_ms: Some(1000),
            closed_at_epoch_ms: None,
            archived_at_epoch_ms: None,
        },
    )
    .await
    .expect("parent");
    let incoming = create_task_node(
        &client,
        "project_alpha",
        "review",
        &TaskNodeInsert {
            parent_task_node_id: None,
            memory_item_id: None,
            task_key: Some(&format!("incoming-child-{suffix}")),
            task_role: Some("proposal"),
            headline: "Incoming child",
            summary: Some("incoming"),
            next_step: Some("route"),
            execution_state: Some("proposed"),
            lifecycle_state: Some("hot"),
            confidence: Some(1.0),
            current_score: None,
            reopened_count: Some(0),
            child_count: Some(0),
            closed_child_count: Some(0),
            pending_return_count: Some(0),
            source_event_ids: Some(&json!([format!("event:incoming-child:{suffix}")])),
            artifact_refs: Some(&json!([format!(
                "artifact://proof/incoming-child/{suffix}"
            )])),
            evidence_span: Some(&json!({"kind":"task_node"})),
            derivation_kind: Some("operator_write"),
            status_payload: &json!({}),
            metadata: &json!({}),
            opened_at_epoch_ms: Some(1200),
            closed_at_epoch_ms: None,
            archived_at_epoch_ms: None,
        },
    )
    .await
    .expect("incoming");
    let decision = create_memory_link_decision(
        &client,
        "project_alpha",
        "review",
        &MemoryLinkDecisionInsert {
            task_node_id: Some(incoming.task_node_id),
            retrieval_trace_id: None,
            candidate_task_node_id: Some(parent.task_node_id),
            decision_outcome: "child",
            legality_passed: true,
            scope_filter_passed: true,
            evidence_sufficient: true,
            classifier_label: Some("create_child"),
            classifier_score: Some(0.91),
            decision_reason: Some("subtask"),
            decision_payload: &json!({"routing":"child"}),
            source_event_ids: Some(&json!([format!("event:child-decision:{suffix}")])),
            artifact_refs: Some(&json!([format!(
                "artifact://proof/child-decision/{suffix}"
            )])),
            message_refs: Some(&json!([format!("message://proof/child-decision/{suffix}")])),
            evidence_span: Some(&json!({"kind":"routing_decision"})),
            derivation_kind: Some("extract"),
            schema_version: Some("memory-link-decision-envelope-v1"),
            recorded_at_epoch_ms: Some(3300),
        },
    )
    .await
    .expect("decision");
    let child_after = get_task_node(&client, incoming.task_node_id)
        .await
        .expect("child after");
    let parent_after = get_task_node(&client, parent.task_node_id)
        .await
        .expect("parent after");
    assert_eq!(child_after.parent_task_node_id, Some(parent.task_node_id));
    assert_eq!(child_after.task_role, "child");
    assert_eq!(child_after.execution_state, "ready");
    assert_eq!(child_after.lifecycle_state, "hot");
    assert_eq!(parent_after.child_count, 1);
    let child_events = client
            .query(
                "SELECT event_kind FROM ami.task_events WHERE task_node_id = $1 AND source_event_id = $2",
                &[&incoming.task_node_id, &format!("memory_link_decision:{}", decision.memory_link_decision_id)],
            )
            .await
            .expect("child events");
    assert_eq!(child_events.len(), 1);
    let event_kind: String = child_events[0].get(0);
    assert_eq!(event_kind, "branched_child");
}

#[tokio::test]
async fn create_memory_link_decision_abstain_and_escalate_materialize_task_events() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = trimmed.split_once('=') {
                unsafe {
                    std::env::set_var(key.trim(), value.trim_matches('\"'));
                }
            }
        }
    }
    unsafe {
        std::env::set_var("AMI_STACK_NAME", "default");
    }
    let cfg = AppConfig::from_env().expect("env config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("unix epoch")
        .as_nanos();
    let task_node = create_task_node(
        &client,
        "project_alpha",
        "review",
        &TaskNodeInsert {
            parent_task_node_id: None,
            memory_item_id: None,
            task_key: Some(&format!("abstain-escalate-{suffix}")),
            task_role: Some("proposal"),
            headline: "Abstain/escalate node",
            summary: Some("routing ambiguity"),
            next_step: Some("wait"),
            execution_state: Some("active"),
            lifecycle_state: Some("hot"),
            confidence: Some(1.0),
            current_score: None,
            reopened_count: Some(0),
            child_count: Some(0),
            closed_child_count: Some(0),
            pending_return_count: Some(0),
            source_event_ids: Some(&json!([format!("event:abstain-escalate:{suffix}")])),
            artifact_refs: Some(&json!([format!(
                "artifact://proof/abstain-escalate/{suffix}"
            )])),
            evidence_span: Some(&json!({"kind":"task_node"})),
            derivation_kind: Some("operator_write"),
            status_payload: &json!({}),
            metadata: &json!({}),
            opened_at_epoch_ms: Some(1000),
            closed_at_epoch_ms: None,
            archived_at_epoch_ms: None,
        },
    )
    .await
    .expect("task node");
    let abstain = create_memory_link_decision(
        &client,
        "project_alpha",
        "review",
        &MemoryLinkDecisionInsert {
            task_node_id: Some(task_node.task_node_id),
            retrieval_trace_id: None,
            candidate_task_node_id: None,
            decision_outcome: "abstain",
            legality_passed: true,
            scope_filter_passed: true,
            evidence_sufficient: false,
            classifier_label: Some("abstain"),
            classifier_score: Some(0.33),
            decision_reason: Some("not enough evidence"),
            decision_payload: &json!({"routing":"abstain"}),
            source_event_ids: Some(&json!([format!("event:abstain:{suffix}")])),
            artifact_refs: Some(&json!([format!("artifact://proof/abstain/{suffix}")])),
            message_refs: Some(&json!([format!("message://proof/abstain/{suffix}")])),
            evidence_span: Some(&json!({"kind":"routing_decision"})),
            derivation_kind: Some("operator_write"),
            schema_version: Some("memory-link-decision-envelope-v1"),
            recorded_at_epoch_ms: Some(4000),
        },
    )
    .await
    .expect("abstain");
    let escalate = create_memory_link_decision(
        &client,
        "project_alpha",
        "review",
        &MemoryLinkDecisionInsert {
            task_node_id: Some(task_node.task_node_id),
            retrieval_trace_id: None,
            candidate_task_node_id: None,
            decision_outcome: "escalate",
            legality_passed: true,
            scope_filter_passed: true,
            evidence_sufficient: false,
            classifier_label: Some("escalate"),
            classifier_score: Some(0.41),
            decision_reason: Some("need raw proof"),
            decision_payload: &json!({
                "routing":"escalate",
                "additional_evidence_request":"attach raw evidence"
            }),
            source_event_ids: Some(&json!([format!("event:escalate:{suffix}")])),
            artifact_refs: Some(&json!([format!("artifact://proof/escalate/{suffix}")])),
            message_refs: Some(&json!([format!("message://proof/escalate/{suffix}")])),
            evidence_span: Some(&json!({"kind":"routing_decision"})),
            derivation_kind: Some("operator_write"),
            schema_version: Some("memory-link-decision-envelope-v1"),
            recorded_at_epoch_ms: Some(4100),
        },
    )
    .await
    .expect("escalate");
    let abstain_rows = client
            .query(
                "SELECT event_kind, event_payload FROM ami.task_events WHERE task_node_id = $1 AND source_event_id = $2",
                &[&task_node.task_node_id, &format!("memory_link_decision:{}", abstain.memory_link_decision_id)],
            )
            .await
            .expect("abstain rows");
    assert_eq!(abstain_rows.len(), 1);
    let abstain_kind: String = abstain_rows[0].get(0);
    let abstain_payload: Value = abstain_rows[0].get(1);
    assert_eq!(abstain_kind, "state_change");
    assert_eq!(abstain_payload["decision_outcome"], json!("abstain"));
    let escalate_rows = client
            .query(
                "SELECT event_kind, event_payload FROM ami.task_events WHERE task_node_id = $1 AND source_event_id = $2",
                &[&task_node.task_node_id, &format!("memory_link_decision:{}", escalate.memory_link_decision_id)],
            )
            .await
            .expect("escalate rows");
    assert_eq!(escalate_rows.len(), 1);
    let escalate_kind: String = escalate_rows[0].get(0);
    let escalate_payload: Value = escalate_rows[0].get(1);
    assert_eq!(escalate_kind, "evidence_request");
    assert_eq!(
        escalate_payload["additional_evidence_request"],
        json!("attach raw evidence")
    );
}

#[tokio::test]
async fn create_memory_link_decision_pending_link_proposal_materializes_evidence_request() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = trimmed.split_once('=') {
                unsafe {
                    std::env::set_var(key.trim(), value.trim_matches('\"'));
                }
            }
        }
    }
    unsafe {
        std::env::set_var("AMI_STACK_NAME", "default");
    }
    let cfg = AppConfig::from_env().expect("env config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("unix epoch")
        .as_nanos();
    let task_node = create_task_node(
        &client,
        "project_alpha",
        "review",
        &TaskNodeInsert {
            parent_task_node_id: None,
            memory_item_id: None,
            task_key: Some(&format!("pending-link-decision-{suffix}")),
            task_role: Some("proposal"),
            headline: "Pending link decision node",
            summary: Some("routing ambiguity"),
            next_step: Some("collect evidence"),
            execution_state: Some("active"),
            lifecycle_state: Some("hot"),
            confidence: Some(1.0),
            current_score: None,
            reopened_count: Some(0),
            child_count: Some(0),
            closed_child_count: Some(0),
            pending_return_count: Some(0),
            source_event_ids: Some(&json!([format!("event:pending-link-decision:{suffix}")])),
            artifact_refs: Some(&json!([format!(
                "artifact://proof/pending-link-decision/{suffix}"
            )])),
            evidence_span: Some(&json!({"kind":"task_node"})),
            derivation_kind: Some("operator_write"),
            status_payload: &json!({}),
            metadata: &json!({}),
            opened_at_epoch_ms: Some(1000),
            closed_at_epoch_ms: None,
            archived_at_epoch_ms: None,
        },
    )
    .await
    .expect("task node");
    let decision = create_memory_link_decision(
        &client,
        "project_alpha",
        "review",
        &MemoryLinkDecisionInsert {
            task_node_id: Some(task_node.task_node_id),
            retrieval_trace_id: None,
            candidate_task_node_id: Some(task_node.task_node_id),
            decision_outcome: "pending_link_proposal",
            legality_passed: true,
            scope_filter_passed: true,
            evidence_sufficient: false,
            classifier_label: Some("pending_link_proposal"),
            classifier_score: Some(0.29),
            decision_reason: Some("not enough evidence to branch safely"),
            decision_payload: &json!({
                "routing":"pending_link_proposal",
                "pending_link_ttl_epoch_ms": 7777,
                "additional_evidence_request":"attach raw diff and latest operator note"
            }),
            source_event_ids: Some(&json!([format!("event:pending-link-decision:{suffix}")])),
            artifact_refs: Some(&json!([format!(
                "artifact://proof/pending-link-decision/{suffix}"
            )])),
            message_refs: Some(&json!([format!(
                "message://proof/pending-link-decision/{suffix}"
            )])),
            evidence_span: Some(&json!({"kind":"routing_decision"})),
            derivation_kind: Some("operator_write"),
            schema_version: Some("memory-link-decision-envelope-v1"),
            recorded_at_epoch_ms: Some(4200),
        },
    )
    .await
    .expect("pending link decision");
    let rows = client
            .query(
                "SELECT event_kind, event_payload FROM ami.task_events WHERE task_node_id = $1 AND source_event_id = $2",
                &[&task_node.task_node_id, &format!("memory_link_decision:{}", decision.memory_link_decision_id)],
            )
            .await
            .expect("pending link decision events");
    assert_eq!(rows.len(), 1);
    let event_kind: String = rows[0].get(0);
    let event_payload: Value = rows[0].get(1);
    assert_eq!(event_kind, "evidence_request");
    assert_eq!(
        event_payload["decision_outcome"],
        json!("pending_link_proposal")
    );
    assert_eq!(event_payload["pending_link_ttl_epoch_ms"], json!(7777));
    assert_eq!(
        event_payload["additional_evidence_request"],
        json!("attach raw diff and latest operator note")
    );
}

#[tokio::test]
async fn create_memory_link_decision_policy_scope_filter_rejects_scope_mismatch() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = trimmed.split_once('=') {
                unsafe {
                    std::env::set_var(key.trim(), value.trim_matches('\"'));
                }
            }
        }
    }

    let cfg = AppConfig::from_env().expect("config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let (_workspace_code, _source_project_code, target_project_code, _transfer_policy_code) =
        create_stage2_import_shared_context(&client, suffix).await;
    let other_project_code = format!("memory_link_decision_other_proj_{suffix}");
    let repo_root = format!("/tmp/{other_project_code}");
    std::fs::create_dir_all(&repo_root).expect("repo root");
    let other_project = upsert_project(
        &client,
        &other_project_code,
        "Memory Link Decision Other Project",
        &repo_root,
        Some("main"),
        "default",
        "project_shared",
        "local_strict",
    )
    .await
    .expect("other project");
    ensure_namespace(
        &client,
        other_project.project_id,
        "default",
        Some("Default"),
        "local_strict",
    )
    .await
    .expect("other namespace");
    let source_event_id = format!("event:link-decision-mismatch:{suffix}");
    let task_node = create_task_node(
            &client,
            &other_project_code,
            "default",
            &TaskNodeInsert {
                parent_task_node_id: None,
                memory_item_id: None,
                task_key: Some(&format!("task-node-other-{suffix}")),
                task_role: Some("proposal"),
                headline: "Task node other project",
                summary: Some("summary"),
                next_step: Some("next"),
                execution_state: Some("active"),
                lifecycle_state: Some("hot"),
                confidence: Some(1.0),
                current_score: None,
                reopened_count: Some(0),
                child_count: Some(0),
                closed_child_count: Some(0),
                pending_return_count: Some(0),
                source_event_ids: Some(&json!([source_event_id.clone()])),
                artifact_refs: Some(&json!([format!(
                    "artifact://proof/link-decision-mismatch/{suffix}"
                )])),
                evidence_span: Some(&json!({"event_id":source_event_id,"kind":"task_node"})),
                derivation_kind: Some("extract"),
                status_payload: &json!({"source_kind":"continuity_handoff","source_event_id":source_event_id}),
                metadata: &json!({"local_path":format!("/tmp/link-decision-mismatch-{suffix}.md")}),
                opened_at_epoch_ms: Some(1000),
                closed_at_epoch_ms: None,
                archived_at_epoch_ms: None,
            },
        )
        .await
        .expect("task node");
    let error = create_memory_link_decision(
        &client,
        &target_project_code,
        "default",
        &MemoryLinkDecisionInsert {
            task_node_id: Some(task_node.task_node_id),
            retrieval_trace_id: None,
            candidate_task_node_id: Some(task_node.task_node_id),
            decision_outcome: "continue",
            legality_passed: true,
            scope_filter_passed: true,
            evidence_sufficient: true,
            classifier_label: Some("continue_existing_branch"),
            classifier_score: Some(0.51),
            decision_reason: Some("mismatch scope"),
            decision_payload: &json!({"routing":"continue"}),
            source_event_ids: Some(&json!([format!("event:link-decision-mismatch:{suffix}")])),
            artifact_refs: Some(&json!([format!(
                "artifact://proof/link-decision-mismatch/{suffix}"
            )])),
            message_refs: Some(&json!([format!("message:link-decision-mismatch:{suffix}")])),
            evidence_span: Some(&json!({"kind":"routing_decision"})),
            derivation_kind: Some("extract"),
            schema_version: Some("memory-link-decision-envelope-v1"),
            recorded_at_epoch_ms: Some(1200),
        },
    )
    .await
    .expect_err("link decision scope mismatch rejected");
    assert!(
        error
            .to_string()
            .contains("memory link decision task node scope does not match")
    );
}

#[tokio::test]
async fn create_memory_link_decision_verification_conflict_check_detects_poisoned_evidence_span() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = trimmed.split_once('=') {
                unsafe {
                    std::env::set_var(key.trim(), value.trim_matches('\"'));
                }
            }
        }
    }

    let cfg = AppConfig::from_env().expect("config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let (_workspace_code, _source_project_code, target_project_code, _transfer_policy_code) =
        create_stage2_import_shared_context(&client, suffix).await;
    let source_event_id = format!("event:link-decision-poison:{suffix}");
    let task_node = create_task_node(
            &client,
            &target_project_code,
            "default",
            &TaskNodeInsert {
                parent_task_node_id: None,
                memory_item_id: None,
                task_key: Some(&format!("task-node-poison-{suffix}")),
                task_role: Some("proposal"),
                headline: "Task node poison decision",
                summary: Some("summary"),
                next_step: Some("next"),
                execution_state: Some("active"),
                lifecycle_state: Some("hot"),
                confidence: Some(1.0),
                current_score: None,
                reopened_count: Some(0),
                child_count: Some(0),
                closed_child_count: Some(0),
                pending_return_count: Some(0),
                source_event_ids: Some(&json!([source_event_id.clone()])),
                artifact_refs: Some(&json!([format!(
                    "artifact://proof/link-decision-poison/{suffix}"
                )])),
                evidence_span: Some(&json!({"event_id":source_event_id,"kind":"task_node"})),
                derivation_kind: Some("extract"),
                status_payload: &json!({"source_kind":"continuity_handoff","source_event_id":source_event_id}),
                metadata: &json!({"local_path":format!("/tmp/link-decision-poison-{suffix}.md")}),
                opened_at_epoch_ms: Some(1000),
                closed_at_epoch_ms: None,
                archived_at_epoch_ms: None,
            },
        )
        .await
        .expect("task node");
    let error = create_memory_link_decision(
        &client,
        &target_project_code,
        "default",
        &MemoryLinkDecisionInsert {
            task_node_id: Some(task_node.task_node_id),
            retrieval_trace_id: None,
            candidate_task_node_id: Some(task_node.task_node_id),
            decision_outcome: "continue",
            legality_passed: true,
            scope_filter_passed: true,
            evidence_sufficient: true,
            classifier_label: Some("continue_existing_branch"),
            classifier_score: Some(0.51),
            decision_reason: Some("poisoned"),
            decision_payload: &json!({"routing":"continue"}),
            source_event_ids: Some(&json!([format!("event:link-decision-poison:{suffix}")])),
            artifact_refs: Some(&json!([format!(
                "artifact://proof/link-decision-poison/{suffix}"
            )])),
            message_refs: Some(&json!([format!("message:link-decision-poison:{suffix}")])),
            evidence_span: Some(&json!({"kind":"routing_decision","poisoned":true})),
            derivation_kind: Some("extract"),
            schema_version: Some("memory-link-decision-envelope-v1"),
            recorded_at_epoch_ms: Some(1200),
        },
    )
    .await
    .expect_err("link decision poisoned rejected");
    assert!(
        error
            .to_string()
            .contains("memory link decision evidence_span is flagged poisoned")
    );
}

#[tokio::test]
async fn create_pending_link_proposal_surfaces_stage2_provenance_fields() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            };
            let Some((key, value)) = trimmed.split_once('=') else {
                continue;
            };
            if std::env::var_os(key).is_none() {
                unsafe {
                    std::env::set_var(key, value);
                }
            }
        }
    }
    unsafe {
        std::env::set_var("AMI_STACK_NAME", "default");
    }
    let cfg = AppConfig::from_env().expect("env config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("unix epoch")
        .as_nanos();
    let source_event_id = format!("event:pending-proposal:{suffix}");
    let task_key = format!("task-node-for-pending-proposal-{suffix}");
    let task_node = create_task_node(
            &client,
            "project_alpha",
            "review",
            &TaskNodeInsert {
                parent_task_node_id: None,
                memory_item_id: None,
                task_key: Some(&task_key),
                task_role: Some("proposal"),
                headline: "Task node for pending proposal proof",
                summary: Some("summary"),
                next_step: Some("next"),
                execution_state: Some("active"),
                lifecycle_state: Some("hot"),
                confidence: Some(1.0),
                current_score: None,
                reopened_count: Some(0),
                child_count: Some(0),
                closed_child_count: Some(0),
                pending_return_count: Some(0),
                source_event_ids: Some(&json!([source_event_id.clone()])),
                artifact_refs: Some(&json!([format!(
                    "artifact://proof/pending-proposal/node/{suffix}"
                )])),
                evidence_span: Some(&json!({"event_id":source_event_id,"kind":"task_node"})),
                derivation_kind: Some("extract"),
                status_payload: &json!({"source_kind":"continuity_handoff","source_event_id":source_event_id}),
                metadata: &json!({"local_path":format!("/tmp/pending-proposal-node-{suffix}.md")}),
                opened_at_epoch_ms: Some(1000),
                closed_at_epoch_ms: None,
                archived_at_epoch_ms: None,
            },
        )
        .await
        .expect("task node");
    let source_event_ids = json!([format!("event:pending-proposal:basis:{suffix}")]);
    let artifact_refs = json!([format!("artifact://proof/pending-proposal/{suffix}")]);
    let message_refs = json!([format!("thread:pending-proposal:{suffix}")]);
    let evidence_span = json!({
        "event_id": source_event_id,
        "candidate_task_key": task_key,
        "kind": "pending_link_proposal",
    });
    let payload = json!({
        "needed": ["more_files", "more_messages"],
        "routing": "pending_link_proposal",
    });
    let proposal = create_pending_link_proposal(
        &client,
        "project_alpha",
        "review",
        &PendingLinkProposalInsert {
            task_node_id: Some(task_node.task_node_id),
            retrieval_trace_id: None,
            candidate_task_node_id: Some(task_node.task_node_id),
            proposal_state: Some("pending"),
            proposal_reason: "insufficient evidence",
            evidence_request: Some("need more raw evidence from current branch"),
            evidence_payload: &payload,
            classifier_score: Some(0.51),
            ttl_epoch_ms: Some(2222),
            source_event_ids: Some(&source_event_ids),
            artifact_refs: Some(&artifact_refs),
            message_refs: Some(&message_refs),
            evidence_span: Some(&evidence_span),
            derivation_kind: Some("extract"),
            schema_version: Some("pending-link-proposal-envelope-v1"),
        },
    )
    .await
    .expect("pending link proposal");
    assert_eq!(proposal.source_event_ids, source_event_ids);
    assert_eq!(proposal.artifact_refs, artifact_refs);
    assert_eq!(proposal.message_refs, message_refs);
    assert_eq!(proposal.evidence_span["kind"], evidence_span["kind"]);
    assert_eq!(
        proposal.evidence_span["stage2_runtime"]["policy_and_scope_filter"]["scope_binding_valid"],
        json!(true)
    );
    assert_eq!(
        proposal.evidence_span["stage2_runtime"]["verification_conflict_check"]["write_allowed"],
        json!(true)
    );
    assert_eq!(proposal.derivation_kind, "extract");
    assert_eq!(proposal.schema_version, "pending-link-proposal-envelope-v1");
}

#[tokio::test]
async fn create_pending_link_proposal_materializes_evidence_request_event() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = trimmed.split_once('=') {
                unsafe {
                    std::env::set_var(key.trim(), value.trim_matches('\"'));
                }
            }
        }
    }
    unsafe {
        std::env::set_var("AMI_STACK_NAME", "default");
    }
    let cfg = AppConfig::from_env().expect("env config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("unix epoch")
        .as_nanos();
    let task_node = create_task_node(
        &client,
        "project_alpha",
        "review",
        &TaskNodeInsert {
            parent_task_node_id: None,
            memory_item_id: None,
            task_key: Some(&format!("proposal-evidence-{suffix}")),
            task_role: Some("proposal"),
            headline: "Pending proposal node",
            summary: Some("pending"),
            next_step: Some("need more evidence"),
            execution_state: Some("active"),
            lifecycle_state: Some("hot"),
            confidence: Some(1.0),
            current_score: None,
            reopened_count: Some(0),
            child_count: Some(0),
            closed_child_count: Some(0),
            pending_return_count: Some(0),
            source_event_ids: Some(&json!([format!("event:proposal-evidence:{suffix}")])),
            artifact_refs: Some(&json!([format!(
                "artifact://proof/proposal-evidence/{suffix}"
            )])),
            evidence_span: Some(&json!({"kind":"task_node"})),
            derivation_kind: Some("operator_write"),
            status_payload: &json!({}),
            metadata: &json!({}),
            opened_at_epoch_ms: Some(1000),
            closed_at_epoch_ms: None,
            archived_at_epoch_ms: None,
        },
    )
    .await
    .expect("task node");
    let proposal = create_pending_link_proposal(
        &client,
        "project_alpha",
        "review",
        &PendingLinkProposalInsert {
            task_node_id: Some(task_node.task_node_id),
            retrieval_trace_id: None,
            candidate_task_node_id: Some(task_node.task_node_id),
            proposal_state: Some("pending"),
            proposal_reason: "need more evidence",
            evidence_request: Some("attach concrete raw evidence"),
            evidence_payload: &json!({"routing":"pending_link_proposal"}),
            classifier_score: Some(0.44),
            ttl_epoch_ms: Some(4444),
            source_event_ids: Some(&json!([format!("event:pending-evidence:{suffix}")])),
            artifact_refs: Some(&json!([format!(
                "artifact://proof/pending-evidence/{suffix}"
            )])),
            message_refs: Some(&json!([format!(
                "message://proof/pending-evidence/{suffix}"
            )])),
            evidence_span: Some(&json!({"kind":"pending_link_proposal"})),
            derivation_kind: Some("extract"),
            schema_version: Some("pending-link-proposal-envelope-v1"),
        },
    )
    .await
    .expect("proposal");
    let evidence_events = client
            .query(
                "SELECT event_kind, event_payload FROM ami.task_events WHERE task_node_id = $1 AND source_event_id = $2",
                &[&task_node.task_node_id, &format!("pending_link_proposal:{}", proposal.pending_link_proposal_id)],
            )
            .await
            .expect("evidence request events");
    assert_eq!(evidence_events.len(), 1);
    let event_kind: String = evidence_events[0].get(0);
    let event_payload: Value = evidence_events[0].get(1);
    assert_eq!(event_kind, "evidence_request");
    assert_eq!(
        event_payload["pending_link_proposal_id"],
        json!(proposal.pending_link_proposal_id)
    );
}

#[tokio::test]
async fn create_pending_link_proposal_policy_scope_filter_rejects_scope_mismatch() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = trimmed.split_once('=') {
                unsafe {
                    std::env::set_var(key.trim(), value.trim_matches('\"'));
                }
            }
        }
    }

    let cfg = AppConfig::from_env().expect("config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let (_workspace_code, _source_project_code, target_project_code, _transfer_policy_code) =
        create_stage2_import_shared_context(&client, suffix).await;
    let other_project_code = format!("pending_link_proposal_other_proj_{suffix}");
    let repo_root = format!("/tmp/{other_project_code}");
    std::fs::create_dir_all(&repo_root).expect("repo root");
    let other_project = upsert_project(
        &client,
        &other_project_code,
        "Pending Link Proposal Other Project",
        &repo_root,
        Some("main"),
        "default",
        "project_shared",
        "local_strict",
    )
    .await
    .expect("other project");
    ensure_namespace(
        &client,
        other_project.project_id,
        "default",
        Some("Default"),
        "local_strict",
    )
    .await
    .expect("other namespace");
    let source_event_id = format!("event:pending-proposal-mismatch:{suffix}");
    let task_node = create_task_node(
            &client,
            &other_project_code,
            "default",
            &TaskNodeInsert {
                parent_task_node_id: None,
                memory_item_id: None,
                task_key: Some(&format!("task-node-other-{suffix}")),
                task_role: Some("proposal"),
                headline: "Task node other project",
                summary: Some("summary"),
                next_step: Some("next"),
                execution_state: Some("active"),
                lifecycle_state: Some("hot"),
                confidence: Some(1.0),
                current_score: None,
                reopened_count: Some(0),
                child_count: Some(0),
                closed_child_count: Some(0),
                pending_return_count: Some(0),
                source_event_ids: Some(&json!([source_event_id.clone()])),
                artifact_refs: Some(&json!([format!(
                    "artifact://proof/pending-proposal-mismatch/{suffix}"
                )])),
                evidence_span: Some(&json!({"event_id":source_event_id,"kind":"task_node"})),
                derivation_kind: Some("extract"),
                status_payload: &json!({"source_kind":"continuity_handoff","source_event_id":source_event_id}),
                metadata: &json!({"local_path":format!("/tmp/pending-proposal-mismatch-{suffix}.md")}),
                opened_at_epoch_ms: Some(1000),
                closed_at_epoch_ms: None,
                archived_at_epoch_ms: None,
            },
        )
        .await
        .expect("task node");
    let error = create_pending_link_proposal(
        &client,
        &target_project_code,
        "default",
        &PendingLinkProposalInsert {
            task_node_id: Some(task_node.task_node_id),
            retrieval_trace_id: None,
            candidate_task_node_id: Some(task_node.task_node_id),
            proposal_state: Some("pending"),
            proposal_reason: "scope mismatch",
            evidence_request: Some("need evidence"),
            evidence_payload: &json!({"routing":"pending"}),
            classifier_score: Some(0.12),
            ttl_epoch_ms: Some(2222),
            source_event_ids: Some(&json!([format!(
                "event:pending-proposal-mismatch:{suffix}"
            )])),
            artifact_refs: Some(&json!([format!(
                "artifact://proof/pending-proposal-mismatch/{suffix}"
            )])),
            message_refs: Some(&json!([format!(
                "message:pending-proposal-mismatch:{suffix}"
            )])),
            evidence_span: Some(&json!({"kind":"pending_link_proposal"})),
            derivation_kind: Some("extract"),
            schema_version: Some("pending-link-proposal-envelope-v1"),
        },
    )
    .await
    .expect_err("pending proposal scope mismatch rejected");
    assert!(
        error
            .to_string()
            .contains("memory link decision task node scope does not match")
    );
}

#[tokio::test]
async fn create_pending_link_proposal_verification_conflict_check_detects_poisoned_evidence_span() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = trimmed.split_once('=') {
                unsafe {
                    std::env::set_var(key.trim(), value.trim_matches('\"'));
                }
            }
        }
    }

    let cfg = AppConfig::from_env().expect("config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let (_workspace_code, _source_project_code, target_project_code, _transfer_policy_code) =
        create_stage2_import_shared_context(&client, suffix).await;
    let source_event_id = format!("event:pending-proposal-poison:{suffix}");
    let task_node = create_task_node(
            &client,
            &target_project_code,
            "default",
            &TaskNodeInsert {
                parent_task_node_id: None,
                memory_item_id: None,
                task_key: Some(&format!("task-node-poison-{suffix}")),
                task_role: Some("proposal"),
                headline: "Task node poison proposal",
                summary: Some("summary"),
                next_step: Some("next"),
                execution_state: Some("active"),
                lifecycle_state: Some("hot"),
                confidence: Some(1.0),
                current_score: None,
                reopened_count: Some(0),
                child_count: Some(0),
                closed_child_count: Some(0),
                pending_return_count: Some(0),
                source_event_ids: Some(&json!([source_event_id.clone()])),
                artifact_refs: Some(&json!([format!(
                    "artifact://proof/pending-proposal-poison/{suffix}"
                )])),
                evidence_span: Some(&json!({"event_id":source_event_id,"kind":"task_node"})),
                derivation_kind: Some("extract"),
                status_payload: &json!({"source_kind":"continuity_handoff","source_event_id":source_event_id}),
                metadata: &json!({"local_path":format!("/tmp/pending-proposal-poison-{suffix}.md")}),
                opened_at_epoch_ms: Some(1000),
                closed_at_epoch_ms: None,
                archived_at_epoch_ms: None,
            },
        )
        .await
        .expect("task node");
    let error = create_pending_link_proposal(
        &client,
        &target_project_code,
        "default",
        &PendingLinkProposalInsert {
            task_node_id: Some(task_node.task_node_id),
            retrieval_trace_id: None,
            candidate_task_node_id: Some(task_node.task_node_id),
            proposal_state: Some("pending"),
            proposal_reason: "poisoned",
            evidence_request: Some("need evidence"),
            evidence_payload: &json!({"routing":"pending"}),
            classifier_score: Some(0.12),
            ttl_epoch_ms: Some(2222),
            source_event_ids: Some(&json!([format!("event:pending-proposal-poison:{suffix}")])),
            artifact_refs: Some(&json!([format!(
                "artifact://proof/pending-proposal-poison/{suffix}"
            )])),
            message_refs: Some(&json!([format!(
                "message:pending-proposal-poison:{suffix}"
            )])),
            evidence_span: Some(&json!({"kind":"pending_link_proposal","poisoned":true})),
            derivation_kind: Some("extract"),
            schema_version: Some("pending-link-proposal-envelope-v1"),
        },
    )
    .await
    .expect_err("pending proposal poisoned rejected");
    assert!(
        error
            .to_string()
            .contains("memory link decision evidence_span is flagged poisoned")
    );
}

#[tokio::test]
async fn insert_artifact_ref_surfaces_stage2_provenance_fields() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            };
            let Some((key, value)) = trimmed.split_once('=') else {
                continue;
            };
            if std::env::var_os(key).is_none() {
                unsafe {
                    std::env::set_var(key, value);
                }
            }
        }
    }
    unsafe {
        std::env::set_var("AMI_STACK_NAME", "default");
    }
    let cfg = AppConfig::from_env().expect("env config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("unix epoch")
        .as_nanos();
    let source_event_ids = json!([format!("event:artifact-ref:{suffix}")]);
    let message_refs = json!([format!("thread:artifact-ref:{suffix}")]);
    let evidence_span = json!({
        "path": format!("state/artifacts/{suffix}.json"),
        "kind": "artifact_ref",
    });
    let metadata = json!({"artifact_role":"context_pack","proof":"stage2"});
    let project_id: Uuid = client
        .query_one(
            "SELECT project_id FROM ami.projects WHERE code = 'project_alpha'",
            &[],
        )
        .await
        .expect("project alpha")
        .get(0);
    let namespace_id: Uuid = client
            .query_one(
                "SELECT namespace_id FROM ami.namespaces WHERE code = 'review' AND project_id = (SELECT project_id FROM ami.projects WHERE code = 'project_alpha')",
                &[],
            )
            .await
            .expect("review namespace")
            .get(0);
    let artifact_ref_id = insert_artifact_ref(
        &client,
        &ArtifactRefInsert {
            project_id,
            namespace_id,
            artifact_kind: "context_pack",
            bucket: "proof-bucket",
            object_key: &format!("proof/context-pack/{suffix}.json"),
            content_type: Some("application/json"),
            source_kind: Some("context_pack_materialization"),
            source_event_ids: Some(&source_event_ids),
            message_refs: Some(&message_refs),
            evidence_span: Some(&evidence_span),
            derivation_kind: Some("extract"),
            schema_version: Some("artifact-ref-envelope-v1"),
            metadata: &metadata,
        },
    )
    .await
    .expect("artifact ref");
    let row = client
            .query_one(
                "SELECT source_kind, source_event_ids, message_refs, evidence_span, derivation_kind, schema_version FROM ami.artifact_refs WHERE artifact_ref_id = $1",
                &[&artifact_ref_id],
            )
            .await
            .expect("artifact row");
    let source_kind: Option<String> = row.get(0);
    let stored_source_event_ids: Value = row.get(1);
    let stored_message_refs: Value = row.get(2);
    let stored_evidence_span: Value = row.get(3);
    let derivation_kind: String = row.get(4);
    let schema_version: String = row.get(5);
    assert_eq!(source_kind.as_deref(), Some("context_pack_materialization"));
    assert_eq!(stored_source_event_ids, source_event_ids);
    assert_eq!(stored_message_refs, message_refs);
    assert_eq!(stored_evidence_span, evidence_span);
    assert_eq!(derivation_kind, "extract");
    assert_eq!(schema_version, "artifact-ref-envelope-v1");
}

#[tokio::test]
async fn create_artifact_ref_surfaces_stage2_runtime_fields() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let Some((key, value)) = trimmed.split_once('=') else {
                continue;
            };
            if std::env::var_os(key).is_none() {
                unsafe {
                    std::env::set_var(key, value);
                }
            }
        }
    }
    unsafe {
        std::env::set_var("AMI_STACK_NAME", "default");
    }
    let cfg = AppConfig::from_env().expect("env config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("unix epoch")
        .as_nanos();
    let project_id: Uuid = client
        .query_one(
            "SELECT project_id FROM ami.projects WHERE code = 'project_alpha'",
            &[],
        )
        .await
        .expect("project alpha")
        .get(0);
    let namespace_id: Uuid = client
            .query_one(
                "SELECT namespace_id FROM ami.namespaces WHERE code = 'review' AND project_id = (SELECT project_id FROM ami.projects WHERE code = 'project_alpha')",
                &[],
            )
            .await
            .expect("review namespace")
            .get(0);

    let artifact_ref = create_artifact_ref(
        &client,
        "project_alpha",
        "review",
        &ArtifactRefInsert {
            project_id,
            namespace_id,
            artifact_kind: "context_pack",
            bucket: "proof-bucket",
            object_key: &format!("proof/create-artifact-ref/{suffix}.json"),
            content_type: Some("application/json"),
            source_kind: Some("context_pack_materialization"),
            source_event_ids: Some(&json!([format!("event:create-artifact-ref:{suffix}")])),
            message_refs: Some(&json!([format!("message:create-artifact-ref:{suffix}")])),
            evidence_span: Some(&json!({"kind":"artifact_ref","surface":"create-artifact-ref"})),
            derivation_kind: Some("extract"),
            schema_version: Some("artifact-ref-envelope-v1"),
            metadata: &json!({"artifact_role":"context_pack","proof":"stage2"}),
        },
    )
    .await
    .expect("artifact ref");

    assert_eq!(artifact_ref.project_code, "project_alpha");
    assert_eq!(artifact_ref.namespace_code, "review");
    assert_eq!(artifact_ref.evidence_span["kind"], json!("artifact_ref"));
    assert_eq!(
        artifact_ref.evidence_span["stage2_runtime"]["policy_and_scope_filter"]["scope_binding_valid"],
        json!(true)
    );
    assert_eq!(
        artifact_ref.evidence_span["stage2_runtime"]["verification_conflict_check"]["write_allowed"],
        json!(true)
    );
}

#[tokio::test]
async fn create_artifact_ref_policy_scope_filter_rejects_namespace_mismatch() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let Some((key, value)) = trimmed.split_once('=') else {
                continue;
            };
            if std::env::var_os(key).is_none() {
                unsafe {
                    std::env::set_var(key, value);
                }
            }
        }
    }
    unsafe {
        std::env::set_var("AMI_STACK_NAME", "default");
    }
    let cfg = AppConfig::from_env().expect("env config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("unix epoch")
        .as_nanos();
    let project_id: Uuid = client
        .query_one(
            "SELECT project_id FROM ami.projects WHERE code = 'project_alpha'",
            &[],
        )
        .await
        .expect("project alpha")
        .get(0);
    let default_namespace_id: Uuid = client
            .query_one(
                "SELECT namespace_id FROM ami.namespaces WHERE code = 'default' AND project_id = (SELECT project_id FROM ami.projects WHERE code = 'project_alpha')",
                &[],
            )
            .await
            .expect("default namespace")
            .get(0);

    let error = create_artifact_ref(
        &client,
        "project_alpha",
        "review",
        &ArtifactRefInsert {
            project_id,
            namespace_id: default_namespace_id,
            artifact_kind: "context_pack",
            bucket: "proof-bucket",
            object_key: &format!("proof/create-artifact-ref-mismatch/{suffix}.json"),
            content_type: Some("application/json"),
            source_kind: Some("context_pack_materialization"),
            source_event_ids: Some(&json!([format!(
                "event:create-artifact-ref-mismatch:{suffix}"
            )])),
            message_refs: Some(&json!([format!(
                "message:create-artifact-ref-mismatch:{suffix}"
            )])),
            evidence_span: Some(&json!({"kind":"artifact_ref","surface":"namespace-mismatch"})),
            derivation_kind: Some("extract"),
            schema_version: Some("artifact-ref-envelope-v1"),
            metadata: &json!({"artifact_role":"context_pack","proof":"stage2"}),
        },
    )
    .await
    .expect_err("namespace mismatch rejected");

    assert!(
        error
            .to_string()
            .contains("artifact ref namespace binding does not match")
    );
}

#[tokio::test]
async fn create_artifact_ref_verification_conflict_check_detects_poisoned_evidence_span() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let Some((key, value)) = trimmed.split_once('=') else {
                continue;
            };
            if std::env::var_os(key).is_none() {
                unsafe {
                    std::env::set_var(key, value);
                }
            }
        }
    }
    unsafe {
        std::env::set_var("AMI_STACK_NAME", "default");
    }
    let cfg = AppConfig::from_env().expect("env config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("unix epoch")
        .as_nanos();
    let project_id: Uuid = client
        .query_one(
            "SELECT project_id FROM ami.projects WHERE code = 'project_alpha'",
            &[],
        )
        .await
        .expect("project alpha")
        .get(0);
    let namespace_id: Uuid = client
            .query_one(
                "SELECT namespace_id FROM ami.namespaces WHERE code = 'review' AND project_id = (SELECT project_id FROM ami.projects WHERE code = 'project_alpha')",
                &[],
            )
            .await
            .expect("review namespace")
            .get(0);

    let error = create_artifact_ref(
        &client,
        "project_alpha",
        "review",
        &ArtifactRefInsert {
            project_id,
            namespace_id,
            artifact_kind: "context_pack",
            bucket: "proof-bucket",
            object_key: &format!("proof/create-artifact-ref-poison/{suffix}.json"),
            content_type: Some("application/json"),
            source_kind: Some("context_pack_materialization"),
            source_event_ids: Some(&json!([format!(
                "event:create-artifact-ref-poison:{suffix}"
            )])),
            message_refs: Some(&json!([format!(
                "message:create-artifact-ref-poison:{suffix}"
            )])),
            evidence_span: Some(&json!({"kind":"artifact_ref","poisoned":true})),
            derivation_kind: Some("extract"),
            schema_version: Some("artifact-ref-envelope-v1"),
            metadata: &json!({"artifact_role":"context_pack","proof":"stage2"}),
        },
    )
    .await
    .expect_err("poisoned artifact ref rejected");

    assert!(
        error
            .to_string()
            .contains("artifact ref evidence_span is flagged poisoned")
    );
}

#[tokio::test]
async fn create_skill_evidence_bundle_surfaces_stage2_provenance_fields() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            };
            let Some((key, value)) = trimmed.split_once('=') else {
                continue;
            };
            if std::env::var_os(key).is_none() {
                unsafe {
                    std::env::set_var(key, value);
                }
            }
        }
    }
    unsafe {
        std::env::set_var("AMI_STACK_NAME", "default");
    }
    let cfg = AppConfig::from_env().expect("env config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("unix epoch")
        .as_nanos();
    let source_event_id = format!("event:skill-bundle:{suffix}");
    let artifact_ref = format!("artifact://proof/skill-bundle/{suffix}");
    let trigger_conditions = vec!["when evidence exists".to_string()];
    let preconditions = vec!["project_alpha".to_string()];
    let execution_steps = vec!["step".to_string()];
    let stop_conditions = vec!["done".to_string()];
    let forbidden_when: Vec<String> = Vec::new();
    let runtime_constraints: Vec<String> = Vec::new();
    let model_constraints: Vec<String> = Vec::new();
    let tool_constraints: Vec<String> = Vec::new();
    let context_constraints: Vec<String> = Vec::new();
    let skill_card = create_skill_card_candidate(
        &client,
        "project_alpha",
        "review",
        &format!("proof.skill.bundle.{suffix}"),
        1,
        "Skill bundle proof",
        "show bundle provenance",
        &trigger_conditions,
        &preconditions,
        &execution_steps,
        &stop_conditions,
        &forbidden_when,
        Some("proof"),
        "project_shared",
        "project",
        &runtime_constraints,
        &model_constraints,
        &tool_constraints,
        &context_constraints,
        &[source_event_id.clone()],
        &[artifact_ref.clone()],
        &json!({"path":format!("docs/skill-bundle-{suffix}.md"),"line_start":1,"line_end":2}),
        None,
        Some("extract"),
    )
    .await
    .expect("skill card");
    let message_refs = json!([format!("thread:skill-bundle:{suffix}")]);
    let evidence_span = json!({
        "path": format!("docs/skill-bundle-{suffix}.md"),
        "line_start": 1,
        "line_end": 2,
        "kind": "skill_evidence_bundle",
    });
    let bundle = create_skill_evidence_bundle(
        &client,
        skill_card.skill_card_id,
        "trace",
        Some("bundle summary"),
        &[source_event_id.clone()],
        &[artifact_ref.clone()],
        Some("skill_trace_capture"),
        Some(&message_refs),
        Some(&evidence_span),
        Some("extract"),
        Some("skill-evidence-bundle-envelope-v1"),
    )
    .await
    .expect("skill evidence bundle");
    assert_eq!(bundle.source_kind.as_deref(), Some("skill_trace_capture"));
    assert_eq!(bundle.source_event_ids, json!([source_event_id]));
    assert_eq!(bundle.artifact_refs, json!([artifact_ref]));
    assert_eq!(bundle.message_refs, message_refs);
    assert_eq!(bundle.evidence_span, evidence_span);
    assert_eq!(bundle.derivation_kind, "extract");
    assert_eq!(bundle.schema_version, "skill-evidence-bundle-envelope-v1");
}

async fn create_stage2_skill_card_for_activity_test(
    client: &Client,
    suffix: u128,
    stem: &str,
) -> SkillCardRecord {
    let source_event_id = format!("event:{stem}:{suffix}");
    let artifact_ref = format!("artifact://proof/{stem}/{suffix}");
    let trigger_conditions = vec![format!("when {stem}")];
    let preconditions = vec!["project_alpha".to_string()];
    let execution_steps = vec!["step".to_string()];
    let stop_conditions = vec!["done".to_string()];
    let forbidden_when: Vec<String> = Vec::new();
    let runtime_constraints: Vec<String> = Vec::new();
    let model_constraints: Vec<String> = Vec::new();
    let tool_constraints: Vec<String> = Vec::new();
    let context_constraints: Vec<String> = Vec::new();
    create_skill_card_candidate(
        client,
        "project_alpha",
        "review",
        &format!("proof.skill.{stem}.{suffix}"),
        1,
        &format!("Skill {stem} proof"),
        "show activity provenance",
        &trigger_conditions,
        &preconditions,
        &execution_steps,
        &stop_conditions,
        &forbidden_when,
        Some("proof"),
        "project_shared",
        "project",
        &runtime_constraints,
        &model_constraints,
        &tool_constraints,
        &context_constraints,
        &[source_event_id],
        &[artifact_ref],
        &json!({"path":format!("docs/{stem}-{suffix}.md"),"line_start":1,"line_end":2}),
        None,
        Some("extract"),
    )
    .await
    .expect("skill card")
}

async fn create_stage2_skill_card_for_constraint_test(
    client: &Client,
    suffix: u128,
    stem: &str,
    runtime_constraints: &[String],
) -> SkillCardRecord {
    let source_event_id = format!("event:{stem}:{suffix}");
    let artifact_ref = format!("artifact://proof/{stem}/{suffix}");
    let trigger_conditions = vec![format!("when {stem}")];
    let preconditions = vec!["project_alpha".to_string()];
    let execution_steps = vec!["step".to_string()];
    let stop_conditions = vec!["done".to_string()];
    let forbidden_when: Vec<String> = Vec::new();
    let model_constraints: Vec<String> = Vec::new();
    let tool_constraints: Vec<String> = Vec::new();
    let context_constraints: Vec<String> = Vec::new();
    create_skill_card_candidate(
        client,
        "project_alpha",
        "review",
        &format!("proof.skill.constraint.{stem}.{suffix}"),
        1,
        &format!("Skill {stem} constraint proof"),
        "show constraint enforcement",
        &trigger_conditions,
        &preconditions,
        &execution_steps,
        &stop_conditions,
        &forbidden_when,
        Some("proof"),
        "project_shared",
        "project",
        runtime_constraints,
        &model_constraints,
        &tool_constraints,
        &context_constraints,
        &[source_event_id],
        &[artifact_ref],
        &json!({"path":format!("docs/{stem}-{suffix}.md"),"line_start":1,"line_end":2}),
        None,
        Some("extract"),
    )
    .await
    .expect("skill card")
}

async fn create_stage2_import_shared_context(
    client: &Client,
    suffix: u128,
) -> (String, String, String, String) {
    let workspace_code = format!("stage2_ws_{suffix}");
    let source_project_code = format!("stage2_src_{suffix}");
    let target_project_code = format!("stage2_tgt_{suffix}");
    let transfer_policy_code = format!("stage2_policy_{suffix}");
    let source_repo_root = format!("/tmp/{source_project_code}");
    let target_repo_root = format!("/tmp/{target_project_code}");

    std::fs::create_dir_all(&source_repo_root).expect("source repo root");
    std::fs::create_dir_all(&target_repo_root).expect("target repo root");

    ensure_workspace(client, &workspace_code, "Stage2 workspace", "active")
        .await
        .expect("workspace");
    upsert_project(
        client,
        &source_project_code,
        "Stage2 source",
        &source_repo_root,
        Some("main"),
        &workspace_code,
        "cross_project_linked",
        "local_strict",
    )
    .await
    .expect("source project");
    upsert_project(
        client,
        &target_project_code,
        "Stage2 target",
        &target_repo_root,
        Some("main"),
        &workspace_code,
        "cross_project_linked",
        "local_strict",
    )
    .await
    .expect("target project");
    ensure_transfer_policy(
        client,
        &workspace_code,
        &transfer_policy_code,
        "Stage2 transfer policy",
        "borrowed_unverified",
        true,
        true,
        true,
        false,
    )
    .await
    .expect("transfer policy");
    add_relation(
        client,
        &source_project_code,
        &target_project_code,
        "shared_codebase",
        Some("knowledge_may_transfer"),
        "facts",
        "cross_project_linked",
        "active",
        false,
        Some(&transfer_policy_code),
        "explicit_foreign",
    )
    .await
    .expect("project relation");
    ensure_access_policy(
        client,
        &workspace_code,
        None,
        None,
        Some(&source_project_code),
        &format!("stage2_import_access_{suffix}"),
        "Stage2 import access",
        "fact",
        "cross_project_linked",
        100,
        true,
        true,
        true,
        true,
        true,
        true,
        true,
        false,
        true,
        true,
        true,
        Some("stage2 import access"),
        "active",
    )
    .await
    .expect("access policy");

    (
        workspace_code,
        source_project_code,
        target_project_code,
        transfer_policy_code,
    )
}

#[tokio::test]
async fn record_skill_trigger_match_surfaces_stage2_provenance_fields() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            };
            let Some((key, value)) = trimmed.split_once('=') else {
                continue;
            };
            if std::env::var_os(key).is_none() {
                unsafe {
                    std::env::set_var(key, value);
                }
            }
        }
    }
    unsafe {
        std::env::set_var("AMI_STACK_NAME", "default");
    }
    let cfg = AppConfig::from_env().expect("env config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("unix epoch")
        .as_nanos();
    let skill_card =
        create_stage2_skill_card_for_activity_test(&client, suffix, "trigger-match").await;
    let source_event_ids = json!([format!("event:trigger-match:{suffix}")]);
    let artifact_refs = json!([format!("artifact://proof/trigger-match/{suffix}")]);
    let message_refs = json!([format!("thread:trigger-match:{suffix}")]);
    let evidence_span = json!({"kind":"skill_trigger_match","input":"trigger input"});
    let record = record_skill_trigger_match(
        &client,
        skill_card.skill_card_id,
        "thread",
        "trigger input",
        true,
        Some("summary"),
        Some("skill_trigger_scan"),
        Some(&source_event_ids),
        Some(&artifact_refs),
        Some(&message_refs),
        Some(&evidence_span),
        Some("extract"),
        Some("skill-trigger-match-envelope-v1"),
    )
    .await
    .expect("trigger match");
    assert_eq!(record.source_kind.as_deref(), Some("skill_trigger_scan"));
    assert_eq!(record.source_event_ids, source_event_ids);
    assert_eq!(record.artifact_refs, artifact_refs);
    assert_eq!(record.message_refs, message_refs);
    assert_eq!(record.evidence_span, evidence_span);
    assert_eq!(record.derivation_kind, "extract");
    assert_eq!(record.schema_version, "skill-trigger-match-envelope-v1");
}

#[tokio::test]
async fn record_skill_trial_run_surfaces_stage2_provenance_fields() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            };
            let Some((key, value)) = trimmed.split_once('=') else {
                continue;
            };
            if std::env::var_os(key).is_none() {
                unsafe {
                    std::env::set_var(key, value);
                }
            }
        }
    }
    unsafe {
        std::env::set_var("AMI_STACK_NAME", "default");
    }
    let cfg = AppConfig::from_env().expect("env config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("unix epoch")
        .as_nanos();
    let skill_card = create_stage2_skill_card_for_activity_test(&client, suffix, "trial-run").await;
    let source_event_ids = json!([format!("event:trial-run:{suffix}")]);
    let artifact_refs = json!([format!("artifact://proof/trial-run/{suffix}")]);
    let message_refs = json!([format!("thread:trial-run:{suffix}")]);
    let evidence_span = json!({"kind":"skill_trial_run","task":"proof task"});
    let record = record_skill_trial_run(
        &client,
        skill_card.skill_card_id,
        "shadow",
        Some("proof task"),
        Some("codex"),
        Some("gpt-5"),
        Some("search"),
        true,
        false,
        "success",
        Some("summary"),
        Some("skill_trial_runtime"),
        Some(&source_event_ids),
        Some(&artifact_refs),
        Some(&message_refs),
        Some(&evidence_span),
        Some("extract"),
        Some("skill-trial-run-envelope-v1"),
    )
    .await
    .expect("trial run");
    assert_eq!(record.source_kind.as_deref(), Some("skill_trial_runtime"));
    assert_eq!(record.source_event_ids, source_event_ids);
    assert_eq!(record.artifact_refs, artifact_refs);
    assert_eq!(record.message_refs, message_refs);
    assert_eq!(record.evidence_span, evidence_span);
    assert_eq!(record.derivation_kind, "extract");
    assert_eq!(record.schema_version, "skill-trial-run-envelope-v1");
}

#[tokio::test]
async fn record_skill_eval_surfaces_stage2_provenance_fields() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            };
            let Some((key, value)) = trimmed.split_once('=') else {
                continue;
            };
            if std::env::var_os(key).is_none() {
                unsafe {
                    std::env::set_var(key, value);
                }
            }
        }
    }
    unsafe {
        std::env::set_var("AMI_STACK_NAME", "default");
    }
    let cfg = AppConfig::from_env().expect("env config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("unix epoch")
        .as_nanos();
    let skill_card = create_stage2_skill_card_for_activity_test(&client, suffix, "eval").await;
    let source_event_id = format!("event:eval:{suffix}");
    let artifact_ref = format!("artifact://proof/eval/{suffix}");
    let message_refs = json!([format!("thread:eval:{suffix}")]);
    let evidence_span = json!({"kind":"skill_eval","scope":"shadow verdict"});
    create_skill_evidence_bundle(
        &client,
        skill_card.skill_card_id,
        "trace",
        Some("bundle"),
        &[source_event_id.clone()],
        &[artifact_ref.clone()],
        Some("skill_eval_evidence"),
        Some(&message_refs),
        Some(&json!({"kind":"bundle","suffix":suffix})),
        Some("extract"),
        Some("skill-evidence-bundle-envelope-v1"),
    )
    .await
    .expect("evidence bundle");
    record_skill_trigger_match(
        &client,
        skill_card.skill_card_id,
        "manual_review",
        "eval trigger",
        true,
        Some("trigger matched"),
        Some("skill_trigger_scan"),
        Some(&json!([format!("event:eval-trigger:{suffix}")])),
        Some(&json!([format!("artifact://proof/eval-trigger/{suffix}")])),
        Some(&message_refs),
        Some(&json!({"kind":"skill_trigger_match","scope":"eval"})),
        Some("extract"),
        Some("skill-trigger-match-envelope-v1"),
    )
    .await
    .expect("trigger match");
    let record = record_skill_eval(
        &client,
        skill_card.skill_card_id,
        "promote_shadow",
        "eval_contour",
        true,
        true,
        true,
        0.5,
        Some("summary"),
        Some("skill_eval_contour"),
        Some(&json!([source_event_id])),
        Some(&json!([artifact_ref])),
        Some(&message_refs),
        Some(&evidence_span),
        Some("extract"),
        Some("skill-eval-envelope-v1"),
    )
    .await
    .expect("skill eval");
    assert_eq!(record.source_kind.as_deref(), Some("skill_eval_contour"));
    assert_eq!(record.message_refs, message_refs);
    assert_eq!(record.evidence_span, evidence_span);
    assert_eq!(record.derivation_kind, "extract");
    assert_eq!(record.schema_version, "skill-eval-envelope-v1");
}

#[tokio::test]
async fn record_skill_reuse_log_surfaces_stage2_provenance_fields() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            };
            let Some((key, value)) = trimmed.split_once('=') else {
                continue;
            };
            if std::env::var_os(key).is_none() {
                unsafe {
                    std::env::set_var(key, value);
                }
            }
        }
    }
    unsafe {
        std::env::set_var("AMI_STACK_NAME", "default");
    }
    let cfg = AppConfig::from_env().expect("env config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("unix epoch")
        .as_nanos();
    let skill_card = create_stage2_skill_card_for_activity_test(&client, suffix, "reuse-log").await;
    let source_event_id = format!("event:reuse-log:{suffix}");
    let artifact_ref = format!("artifact://proof/reuse-log/{suffix}");
    let message_refs = json!([format!("thread:reuse-log:{suffix}")]);
    let evidence_span = json!({
        "kind":"skill_reuse_log",
        "task":"proof task",
        "matched": true,
        "applied": true
    });
    let record = record_skill_reuse_log(
        &client,
        skill_card.skill_card_id,
        "shadow",
        Some("proof task"),
        "success",
        Some("summary"),
        &[source_event_id.clone()],
        &[artifact_ref.clone()],
        Some("skill_reuse_runtime"),
        Some(&message_refs),
        Some(&evidence_span),
        Some("extract"),
        Some("skill-reuse-log-envelope-v1"),
    )
    .await
    .expect("reuse log");
    assert_eq!(record.source_kind.as_deref(), Some("skill_reuse_runtime"));
    assert_eq!(record.source_event_ids, json!([source_event_id]));
    assert_eq!(record.artifact_refs, json!([artifact_ref]));
    assert_eq!(record.message_refs, message_refs);
    assert_eq!(record.evidence_span, evidence_span);
    assert_eq!(record.derivation_kind, "extract");
    assert_eq!(record.schema_version, "skill-reuse-log-envelope-v1");
}

#[tokio::test]
async fn record_skill_reuse_log_requires_runtime_constraint_match() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            };
            let Some((key, value)) = trimmed.split_once('=') else {
                continue;
            };
            if std::env::var_os(key).is_none() {
                unsafe {
                    std::env::set_var(key, value);
                }
            }
        }
    }
    unsafe {
        std::env::set_var("AMI_STACK_NAME", "default");
    }
    let cfg = AppConfig::from_env().expect("env config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("unix epoch")
        .as_nanos();
    let skill_card = create_stage2_skill_card_for_constraint_test(
        &client,
        suffix,
        "reuse-runtime",
        &["codex".to_string()],
    )
    .await;
    let source_event_id = format!("event:reuse-runtime:{suffix}");
    let artifact_ref = format!("artifact://proof/reuse-runtime/{suffix}");
    let message_refs = json!([format!("thread:reuse-runtime:{suffix}")]);
    let error = record_skill_reuse_log(
        &client,
        skill_card.skill_card_id,
        "shadow",
        Some("proof task"),
        "success",
        Some("summary"),
        &[source_event_id.clone()],
        &[artifact_ref.clone()],
        Some("skill_reuse_runtime"),
        Some(&message_refs),
        Some(&json!({"kind":"skill_reuse_log"})),
        Some("extract"),
        Some("skill-reuse-log-envelope-v1"),
    )
    .await
    .expect_err("runtime constraint should reject missing runtime");
    assert!(error.to_string().contains("skill reuse log"));

    let evidence_span = json!({
        "kind":"skill_reuse_log",
        "runtime":"codex",
        "matched": true,
        "applied": true
    });
    let record = record_skill_reuse_log(
        &client,
        skill_card.skill_card_id,
        "shadow",
        Some("proof task"),
        "success",
        Some("summary"),
        &[source_event_id],
        &[artifact_ref],
        Some("skill_reuse_runtime"),
        Some(&message_refs),
        Some(&evidence_span),
        Some("extract"),
        Some("skill-reuse-log-envelope-v1"),
    )
    .await
    .expect("reuse log with runtime");
    assert_eq!(record.evidence_span, evidence_span);
}

#[tokio::test]
async fn record_skill_reuse_log_rejects_unknown_mode() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            };
            let Some((key, value)) = trimmed.split_once('=') else {
                continue;
            };
            if std::env::var_os(key).is_none() {
                unsafe {
                    std::env::set_var(key, value);
                }
            }
        }
    }
    unsafe {
        std::env::set_var("AMI_STACK_NAME", "default");
    }
    let cfg = AppConfig::from_env().expect("env config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("unix epoch")
        .as_nanos();
    let skill_card =
        create_stage2_skill_card_for_activity_test(&client, suffix, "reuse-mode").await;
    let source_event_id = format!("event:reuse-mode:{suffix}");
    let artifact_ref = format!("artifact://proof/reuse-mode/{suffix}");
    let message_refs = json!([format!("thread:reuse-mode:{suffix}")]);
    let error = record_skill_reuse_log(
        &client,
        skill_card.skill_card_id,
        "unexpected_mode",
        Some("proof task"),
        "neutral",
        Some("summary"),
        &[source_event_id],
        &[artifact_ref],
        Some("skill_reuse_runtime"),
        Some(&message_refs),
        Some(&json!({"kind":"skill_reuse_log"})),
        Some("extract"),
        Some("skill-reuse-log-envelope-v1"),
    )
    .await
    .expect_err("invalid reuse mode should fail");
    assert!(error.to_string().contains("reuse_mode"));
}

#[tokio::test]
async fn record_skill_reuse_log_verified_requires_match_and_apply() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            };
            let Some((key, value)) = trimmed.split_once('=') else {
                continue;
            };
            if std::env::var_os(key).is_none() {
                unsafe {
                    std::env::set_var(key, value);
                }
            }
        }
    }
    unsafe {
        std::env::set_var("AMI_STACK_NAME", "default");
    }
    let cfg = AppConfig::from_env().expect("env config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("unix epoch")
        .as_nanos();
    let skill_card =
        create_stage2_skill_card_for_activity_test(&client, suffix, "reuse-verified").await;
    let source_event_id = format!("event:reuse-verified:{suffix}");
    let artifact_ref = format!("artifact://proof/reuse-verified/{suffix}");
    let message_refs = json!([format!("thread:reuse-verified:{suffix}")]);
    let error = record_skill_reuse_log(
        &client,
        skill_card.skill_card_id,
        "verified",
        Some("proof task"),
        "neutral",
        Some("summary"),
        &[source_event_id.clone()],
        &[artifact_ref.clone()],
        Some("skill_reuse_runtime"),
        Some(&message_refs),
        Some(&json!({"kind":"skill_reuse_log"})),
        Some("extract"),
        Some("skill-reuse-log-envelope-v1"),
    )
    .await
    .expect_err("verified reuse should require matched/applied");
    assert!(error.to_string().contains("reuse_mode=verified"));

    let evidence_span = json!({
        "kind":"skill_reuse_log",
        "matched": true,
        "applied": true
    });
    let record = record_skill_reuse_log(
        &client,
        skill_card.skill_card_id,
        "verified",
        Some("proof task"),
        "neutral",
        Some("summary"),
        &[source_event_id],
        &[artifact_ref],
        Some("skill_reuse_runtime"),
        Some(&message_refs),
        Some(&evidence_span),
        Some("extract"),
        Some("skill-reuse-log-envelope-v1"),
    )
    .await
    .expect("verified reuse allowed");
    assert_eq!(record.evidence_span, evidence_span);
}

#[tokio::test]
async fn record_skill_trial_run_requires_runtime_constraint_match() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            };
            let Some((key, value)) = trimmed.split_once('=') else {
                continue;
            };
            if std::env::var_os(key).is_none() {
                unsafe {
                    std::env::set_var(key, value);
                }
            }
        }
    }
    unsafe {
        std::env::set_var("AMI_STACK_NAME", "default");
    }
    let cfg = AppConfig::from_env().expect("env config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("unix epoch")
        .as_nanos();
    let skill_card = create_stage2_skill_card_for_constraint_test(
        &client,
        suffix,
        "trial-runtime",
        &["codex".to_string()],
    )
    .await;
    let message_refs = json!([format!("thread:trial-runtime:{suffix}")]);
    let error = record_skill_trial_run(
        &client,
        skill_card.skill_card_id,
        "shadow",
        Some("proof task"),
        None,
        None,
        None,
        true,
        false,
        "success",
        Some("summary"),
        Some("trial_runtime"),
        Some(&json!([format!("event:trial-runtime:{suffix}")])),
        Some(&json!([format!("artifact://proof/trial-runtime/{suffix}")])),
        Some(&message_refs),
        Some(&json!({"kind":"skill_trial_run"})),
        Some("extract"),
        Some("skill-trial-run-envelope-v1"),
    )
    .await
    .expect_err("runtime constraint should reject missing runtime");
    assert!(error.to_string().contains("skill trial run"));

    let evidence_span = json!({
        "kind":"skill_trial_run",
        "runtime":"codex"
    });
    let record = record_skill_trial_run(
        &client,
        skill_card.skill_card_id,
        "shadow",
        Some("proof task"),
        None,
        None,
        None,
        true,
        false,
        "success",
        Some("summary"),
        Some("trial_runtime"),
        Some(&json!([format!("event:trial-runtime:{suffix}")])),
        Some(&json!([format!("artifact://proof/trial-runtime/{suffix}")])),
        Some(&message_refs),
        Some(&evidence_span),
        Some("extract"),
        Some("skill-trial-run-envelope-v1"),
    )
    .await
    .expect("trial run with runtime");
    assert_eq!(record.evidence_span, evidence_span);
}

#[test]
fn import_and_shared_surface_validation_rejects_basis_free_extract() {
    let error = validate_stage2_basis(
        "import packet",
        "extract",
        &json!([]),
        &json!([]),
        &json!([]),
        &json!({}),
    )
    .expect_err("basis-free import/shared surface rejected");
    assert!(
        error.to_string().contains(
            "import packet requires recorded basis unless derivation_kind=operator_write"
        )
    );
}

#[tokio::test]
async fn create_import_packet_surfaces_stage2_provenance_fields() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = trimmed.split_once('=') {
                unsafe {
                    std::env::set_var(key.trim(), value.trim_matches('\"'));
                }
            }
        }
    }

    let cfg = AppConfig::from_env().expect("config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let (_workspace_code, source_project_code, target_project_code, transfer_policy_code) =
        create_stage2_import_shared_context(&client, suffix).await;
    let source_event_ids = json!([format!("event:import-packet:{suffix}")]);
    let artifact_refs = vec![format!("artifact://proof/import-packet/{suffix}")];
    let message_refs = json!([format!("message:import-packet:{suffix}")]);
    let evidence_span = json!({"kind":"import_packet","reason":"stage2-proof"});
    let packet = create_import_packet(
        &client,
        &source_project_code,
        &target_project_code,
        Some(&transfer_policy_code),
        None,
        "borrowed_unverified",
        Some("import summary"),
        Some("stage2 import reason"),
        "imported",
        "proposed",
        "unverified",
        "borrowed",
        false,
        &[format!("memory-item:{suffix}")],
        &artifact_refs,
        Some("project_link_import"),
        Some(&source_event_ids),
        Some(&message_refs),
        Some(&evidence_span),
        Some("import"),
        Some("import-packet-envelope-v1"),
    )
    .await
    .expect("import packet");

    assert_eq!(packet.source_project_code, source_project_code);
    assert_eq!(packet.target_project_code, target_project_code);
    assert_eq!(
        packet.transfer_policy_code.as_deref(),
        Some(transfer_policy_code.as_str())
    );
    assert_eq!(packet.source_kind.as_deref(), Some("project_link_import"));
    assert_eq!(packet.source_event_ids, source_event_ids);
    assert_eq!(packet.artifact_refs, json!(artifact_refs));
    assert_eq!(packet.message_refs, message_refs);
    assert_eq!(packet.evidence_span["kind"], evidence_span["kind"]);
    assert_eq!(packet.evidence_span["reason"], evidence_span["reason"]);
    assert_eq!(
        packet.evidence_span["stage2_runtime"]["policy_and_scope_filter"]["scope_allowed"],
        json!(true)
    );
    assert_eq!(
        packet.evidence_span["stage2_runtime"]["verification_conflict_check"]["write_allowed"],
        json!(true)
    );
    assert_eq!(packet.derivation_kind, "import");
    assert_eq!(packet.schema_version, "import-packet-envelope-v1");
}

#[tokio::test]
async fn ensure_shared_asset_surfaces_stage2_provenance_fields() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = trimmed.split_once('=') {
                unsafe {
                    std::env::set_var(key.trim(), value.trim_matches('\"'));
                }
            }
        }
    }

    let cfg = AppConfig::from_env().expect("config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let (workspace_code, source_project_code, _target_project_code, transfer_policy_code) =
        create_stage2_import_shared_context(&client, suffix).await;
    let source_event_ids = json!([format!("event:shared-asset:{suffix}")]);
    let artifact_refs = json!([format!("artifact://proof/shared-asset/{suffix}")]);
    let message_refs = json!([format!("message:shared-asset:{suffix}")]);
    let evidence_span = json!({"kind":"shared_asset","path":"docs/shared.md"});
    let asset = ensure_shared_asset(
        &client,
        &workspace_code,
        &format!("asset_{suffix}"),
        "Stage2 asset",
        "artifact",
        Some(&source_project_code),
        Some(&transfer_policy_code),
        "cross_project_linked",
        "active",
        Some("shared_asset_extract"),
        Some(&source_event_ids),
        Some(&artifact_refs),
        Some(&message_refs),
        Some(&evidence_span),
        Some("extract"),
        Some("shared-asset-envelope-v1"),
    )
    .await
    .expect("shared asset");

    assert_eq!(asset.workspace_code, workspace_code);
    assert_eq!(
        asset.source_project_code.as_deref(),
        Some(source_project_code.as_str())
    );
    assert_eq!(
        asset.transfer_policy_code.as_deref(),
        Some(transfer_policy_code.as_str())
    );
    assert_eq!(asset.source_kind.as_deref(), Some("shared_asset_extract"));
    assert_eq!(asset.source_event_ids, source_event_ids);
    assert_eq!(asset.artifact_refs, artifact_refs);
    assert_eq!(asset.message_refs, message_refs);
    assert_eq!(asset.evidence_span["kind"], evidence_span["kind"]);
    assert_eq!(asset.evidence_span["path"], evidence_span["path"]);
    assert_eq!(
        asset.evidence_span["stage2_runtime"]["policy_and_scope_filter"]["transfer_policy_required"],
        json!(true)
    );
    assert_eq!(
        asset.evidence_span["stage2_runtime"]["verification_conflict_check"]["write_allowed"],
        json!(true)
    );
    assert_eq!(asset.derivation_kind, "extract");
    assert_eq!(asset.schema_version, "shared-asset-envelope-v1");
}

#[tokio::test]
async fn ensure_shared_asset_policy_scope_filter_requires_transfer_policy_for_cross_project_visibility()
 {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = trimmed.split_once('=') {
                unsafe {
                    std::env::set_var(key.trim(), value.trim_matches('\"'));
                }
            }
        }
    }

    let cfg = AppConfig::from_env().expect("config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let (workspace_code, source_project_code, _target_project_code, _transfer_policy_code) =
        create_stage2_import_shared_context(&client, suffix).await;
    let error = ensure_shared_asset(
        &client,
        &workspace_code,
        &format!("missing_policy_asset_{suffix}"),
        "Missing policy asset",
        "artifact",
        Some(&source_project_code),
        None,
        "cross_project_linked",
        "active",
        Some("shared_asset_extract"),
        Some(&json!([format!("event:missing-policy:{suffix}")])),
        Some(&json!([format!(
            "artifact://proof/missing-policy/{suffix}"
        )])),
        Some(&json!([format!("message:missing-policy:{suffix}")])),
        Some(&json!({"kind":"shared_asset","case":"missing-transfer-policy"})),
        Some("extract"),
        Some("shared-asset-envelope-v1"),
    )
    .await
    .expect_err("shared asset without transfer policy rejected");
    assert!(error.to_string().contains("requires transfer_policy"));
}

#[tokio::test]
async fn ensure_shared_asset_verification_conflict_check_detects_poisoned_evidence_span() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = trimmed.split_once('=') {
                unsafe {
                    std::env::set_var(key.trim(), value.trim_matches('\"'));
                }
            }
        }
    }

    let cfg = AppConfig::from_env().expect("config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let (workspace_code, source_project_code, _target_project_code, transfer_policy_code) =
        create_stage2_import_shared_context(&client, suffix).await;
    let error = ensure_shared_asset(
        &client,
        &workspace_code,
        &format!("poisoned_asset_{suffix}"),
        "Poisoned asset",
        "artifact",
        Some(&source_project_code),
        Some(&transfer_policy_code),
        "cross_project_linked",
        "active",
        Some("shared_asset_extract"),
        Some(&json!([format!("event:poisoned-asset:{suffix}")])),
        Some(&json!([format!(
            "artifact://proof/poisoned-asset/{suffix}"
        )])),
        Some(&json!([format!("message:poisoned-asset:{suffix}")])),
        Some(&json!({"kind":"shared_asset","poisoned":true})),
        Some("extract"),
        Some("shared-asset-envelope-v1"),
    )
    .await
    .expect_err("poisoned shared asset rejected");
    assert!(error.to_string().contains("flagged poisoned"));
}

#[tokio::test]
async fn ensure_shared_asset_surfaces_stage2_provenance_fields_for_org_global() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = trimmed.split_once('=') {
                unsafe {
                    std::env::set_var(key.trim(), value.trim_matches('\"'));
                }
            }
        }
    }

    let cfg = AppConfig::from_env().expect("config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let (workspace_code, source_project_code, _target_project_code, transfer_policy_code) =
        create_stage2_import_shared_context(&client, suffix).await;
    let evidence_span = json!({"kind":"shared_asset","scope":"org_global"});
    let asset = ensure_shared_asset(
        &client,
        &workspace_code,
        &format!("org_global_asset_{suffix}"),
        "Org-global asset",
        "artifact",
        Some(&source_project_code),
        Some(&transfer_policy_code),
        "org_global",
        "active",
        Some("shared_asset_extract"),
        Some(&json!([format!("event:org-global-asset:{suffix}")])),
        Some(&json!([format!(
            "artifact://proof/org-global-asset/{suffix}"
        )])),
        Some(&json!([format!("message:org-global-asset:{suffix}")])),
        Some(&evidence_span),
        Some("extract"),
        Some("shared-asset-envelope-v1"),
    )
    .await
    .expect("org_global shared asset");

    assert_eq!(asset.visibility_scope, "org_global");
    assert_eq!(
        asset.transfer_policy_code.as_deref(),
        Some(transfer_policy_code.as_str())
    );
    assert_eq!(
        asset.evidence_span["stage2_runtime"]["policy_and_scope_filter"]["visibility_scope"],
        json!("org_global")
    );
    assert_eq!(
        asset.evidence_span["stage2_runtime"]["policy_and_scope_filter"]["transfer_policy_required"],
        json!(true)
    );
    assert_eq!(
        asset.evidence_span["stage2_runtime"]["policy_and_scope_filter"]["scope_allowed"],
        json!(true)
    );
}

#[tokio::test]
async fn bind_shared_asset_to_project_surfaces_stage2_provenance_fields() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = trimmed.split_once('=') {
                unsafe {
                    std::env::set_var(key.trim(), value.trim_matches('\"'));
                }
            }
        }
    }

    let cfg = AppConfig::from_env().expect("config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let (workspace_code, source_project_code, target_project_code, transfer_policy_code) =
        create_stage2_import_shared_context(&client, suffix).await;
    let asset = ensure_shared_asset(
        &client,
        &workspace_code,
        &format!("binding_asset_{suffix}"),
        "Binding asset",
        "component",
        Some(&source_project_code),
        Some(&transfer_policy_code),
        "cross_project_linked",
        "active",
        Some("shared_asset_extract"),
        Some(&json!([format!("event:binding-asset:{suffix}")])),
        Some(&json!([format!("artifact://proof/binding-asset/{suffix}")])),
        Some(&json!([format!("message:binding-asset:{suffix}")])),
        Some(&json!({"kind":"shared_asset","binding":"source"})),
        Some("extract"),
        Some("shared-asset-envelope-v1"),
    )
    .await
    .expect("shared asset");

    let source_event_ids = json!([format!("event:shared-asset-binding:{suffix}")]);
    let artifact_refs = json!([format!("artifact://proof/shared-asset-binding/{suffix}")]);
    let message_refs = json!([format!("message:shared-asset-binding:{suffix}")]);
    let evidence_span = json!({"kind":"shared_asset_project","binding":"consumer"});
    bind_shared_asset_to_project(
        &client,
        &asset.code,
        &target_project_code,
        "consumer",
        Some("shared_asset_binding"),
        Some(&source_event_ids),
        Some(&artifact_refs),
        Some(&message_refs),
        Some(&evidence_span),
        Some("extract"),
        Some("shared-asset-project-binding-v1"),
    )
    .await
    .expect("binding");

    let row = client
        .query_one(
            r#"
                SELECT
                    sap.source_kind,
                    sap.source_event_ids,
                    sap.artifact_refs,
                    sap.message_refs,
                    sap.evidence_span,
                    sap.derivation_kind,
                    sap.schema_version
                FROM ami.shared_asset_projects sap
                INNER JOIN ami.shared_assets sa ON sa.shared_asset_id = sap.shared_asset_id
                INNER JOIN ami.projects p ON p.project_id = sap.project_id
                WHERE sa.code = $1
                  AND p.code = $2
                "#,
            &[&asset.code, &target_project_code],
        )
        .await
        .expect("binding row");
    let source_kind: Option<String> = row.get(0);
    assert_eq!(source_kind.as_deref(), Some("shared_asset_binding"));
    assert_eq!(row.get::<_, Value>(1), source_event_ids);
    assert_eq!(row.get::<_, Value>(2), artifact_refs);
    assert_eq!(row.get::<_, Value>(3), message_refs);
    let stored_evidence_span = row.get::<_, Value>(4);
    assert_eq!(stored_evidence_span["kind"], evidence_span["kind"]);
    assert_eq!(stored_evidence_span["binding"], evidence_span["binding"]);
    assert_eq!(
        stored_evidence_span["stage2_runtime"]["policy_and_scope_filter"]["workspace_match"],
        json!(true)
    );
    assert_eq!(
        stored_evidence_span["stage2_runtime"]["verification_conflict_check"]["write_allowed"],
        json!(true)
    );
    assert_eq!(row.get::<_, String>(5), "extract");
    assert_eq!(row.get::<_, String>(6), "shared-asset-project-binding-v1");
}

#[tokio::test]
async fn bind_shared_asset_to_project_allows_org_global_within_workspace() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = trimmed.split_once('=') {
                unsafe {
                    std::env::set_var(key.trim(), value.trim_matches('\"'));
                }
            }
        }
    }

    let cfg = AppConfig::from_env().expect("config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let (workspace_code, source_project_code, target_project_code, transfer_policy_code) =
        create_stage2_import_shared_context(&client, suffix).await;
    let asset = ensure_shared_asset(
        &client,
        &workspace_code,
        &format!("org_global_binding_asset_{suffix}"),
        "Org-global binding asset",
        "artifact",
        Some(&source_project_code),
        Some(&transfer_policy_code),
        "org_global",
        "active",
        Some("shared_asset_extract"),
        Some(&json!([format!("event:org-global-binding-asset:{suffix}")])),
        Some(&json!([format!(
            "artifact://proof/org-global-binding-asset/{suffix}"
        )])),
        Some(&json!([format!(
            "message:org-global-binding-asset:{suffix}"
        )])),
        Some(&json!({"kind":"shared_asset","scope":"org_global"})),
        Some("extract"),
        Some("shared-asset-envelope-v1"),
    )
    .await
    .expect("shared asset");

    bind_shared_asset_to_project(
        &client,
        &asset.code,
        &target_project_code,
        "consumer",
        Some("shared_asset_binding"),
        Some(&json!([format!("event:org-global-binding:{suffix}")])),
        Some(&json!([format!(
            "artifact://proof/org-global-binding/{suffix}"
        )])),
        Some(&json!([format!("message:org-global-binding:{suffix}")])),
        Some(&json!({"kind":"shared_asset_project","scope":"org_global"})),
        Some("extract"),
        Some("shared-asset-project-binding-v1"),
    )
    .await
    .expect("org_global binding");

    let row = client
        .query_one(
            r#"
                SELECT sap.binding_kind, sap.evidence_span
                FROM ami.shared_asset_projects sap
                INNER JOIN ami.shared_assets sa ON sa.shared_asset_id = sap.shared_asset_id
                INNER JOIN ami.projects p ON p.project_id = sap.project_id
                WHERE sa.code = $1
                  AND p.code = $2
                "#,
            &[&asset.code, &target_project_code],
        )
        .await
        .expect("binding row");
    assert_eq!(row.get::<_, String>(0), "consumer");
    let stored_evidence_span = row.get::<_, Value>(1);
    assert_eq!(
        stored_evidence_span["stage2_runtime"]["policy_and_scope_filter"]["workspace_match"],
        json!(true)
    );
    assert_eq!(
        stored_evidence_span["stage2_runtime"]["verification_conflict_check"]["write_allowed"],
        json!(true)
    );
}

#[tokio::test]
async fn bind_shared_asset_to_project_rejects_cross_workspace_binding() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = trimmed.split_once('=') {
                unsafe {
                    std::env::set_var(key.trim(), value.trim_matches('\"'));
                }
            }
        }
    }

    let cfg = AppConfig::from_env().expect("config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let (workspace_a, source_project_a, _target_project_a, transfer_policy_a) =
        create_stage2_import_shared_context(&client, suffix).await;
    let (_workspace_b, _source_project_b, target_project_b, _transfer_policy_b) =
        create_stage2_import_shared_context(&client, suffix + 1).await;
    let asset = ensure_shared_asset(
        &client,
        &workspace_a,
        &format!("cross_workspace_asset_{suffix}"),
        "Cross-workspace asset",
        "artifact",
        Some(&source_project_a),
        Some(&transfer_policy_a),
        "cross_project_linked",
        "active",
        Some("shared_asset_extract"),
        Some(&json!([format!("event:cross-workspace-asset:{suffix}")])),
        Some(&json!([format!(
            "artifact://proof/cross-workspace-asset/{suffix}"
        )])),
        Some(&json!([format!("message:cross-workspace-asset:{suffix}")])),
        Some(&json!({"kind":"shared_asset","scope":"workspace-a"})),
        Some("extract"),
        Some("shared-asset-envelope-v1"),
    )
    .await
    .expect("shared asset");

    let error = bind_shared_asset_to_project(
        &client,
        &asset.code,
        &target_project_b,
        "consumer",
        Some("shared_asset_binding"),
        Some(&json!([format!("event:cross-workspace-binding:{suffix}")])),
        Some(&json!([format!(
            "artifact://proof/cross-workspace-binding/{suffix}"
        )])),
        Some(&json!([format!(
            "message:cross-workspace-binding:{suffix}"
        )])),
        Some(&json!({"kind":"shared_asset_project","scope":"cross-workspace"})),
        Some("extract"),
        Some("shared-asset-project-binding-v1"),
    )
    .await
    .expect_err("cross-workspace binding rejected");
    assert!(error.to_string().contains("across workspaces"));
}

#[tokio::test]
async fn bind_shared_asset_to_project_uses_workspace_scoped_asset_lookup() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = trimmed.split_once('=') {
                unsafe {
                    std::env::set_var(key.trim(), value.trim_matches('\"'));
                }
            }
        }
    }

    let cfg = AppConfig::from_env().expect("config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let shared_code = format!("duplicate_asset_code_{suffix}");
    let (workspace_a, source_project_a, target_project_a, transfer_policy_a) =
        create_stage2_import_shared_context(&client, suffix).await;
    let (workspace_b, source_project_b, _target_project_b, transfer_policy_b) =
        create_stage2_import_shared_context(&client, suffix + 1).await;

    let asset_a = ensure_shared_asset(
        &client,
        &workspace_a,
        &shared_code,
        "Duplicate asset workspace A",
        "artifact",
        Some(&source_project_a),
        Some(&transfer_policy_a),
        "org_global",
        "active",
        Some("shared_asset_extract"),
        Some(&json!([format!("event:duplicate-asset-a:{suffix}")])),
        Some(&json!([format!(
            "artifact://proof/duplicate-asset-a/{suffix}"
        )])),
        Some(&json!([format!("message:duplicate-asset-a:{suffix}")])),
        Some(&json!({"kind":"shared_asset","workspace":"a"})),
        Some("extract"),
        Some("shared-asset-envelope-v1"),
    )
    .await
    .expect("workspace a asset");
    let asset_b = ensure_shared_asset(
        &client,
        &workspace_b,
        &shared_code,
        "Duplicate asset workspace B",
        "artifact",
        Some(&source_project_b),
        Some(&transfer_policy_b),
        "org_global",
        "active",
        Some("shared_asset_extract"),
        Some(&json!([format!("event:duplicate-asset-b:{suffix}")])),
        Some(&json!([format!(
            "artifact://proof/duplicate-asset-b/{suffix}"
        )])),
        Some(&json!([format!("message:duplicate-asset-b:{suffix}")])),
        Some(&json!({"kind":"shared_asset","workspace":"b"})),
        Some("extract"),
        Some("shared-asset-envelope-v1"),
    )
    .await
    .expect("workspace b asset");

    bind_shared_asset_to_project(
        &client,
        &shared_code,
        &target_project_a,
        "consumer",
        Some("shared_asset_binding"),
        Some(&json!([format!("event:duplicate-binding:{suffix}")])),
        Some(&json!([format!(
            "artifact://proof/duplicate-binding/{suffix}"
        )])),
        Some(&json!([format!("message:duplicate-binding:{suffix}")])),
        Some(&json!({"kind":"shared_asset_project","scope":"workspace-scoped-lookup"})),
        Some("extract"),
        Some("shared-asset-project-binding-v1"),
    )
    .await
    .expect("workspace-scoped binding");

    let rows = client
        .query(
            r#"
                SELECT
                    w.code,
                    sa.display_name,
                    sap.binding_kind
                FROM ami.shared_asset_projects sap
                INNER JOIN ami.shared_assets sa ON sa.shared_asset_id = sap.shared_asset_id
                INNER JOIN ami.workspaces w ON w.workspace_id = sa.workspace_id
                INNER JOIN ami.projects p ON p.project_id = sap.project_id
                WHERE sa.code = $1
                  AND p.code = $2
                ORDER BY w.code
                "#,
            &[&shared_code, &target_project_a],
        )
        .await
        .expect("binding rows");

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].get::<_, String>(0), workspace_a);
    assert_eq!(rows[0].get::<_, String>(1), asset_a.display_name);
    assert_eq!(rows[0].get::<_, String>(2), "consumer");
    assert_ne!(asset_a.workspace_code, asset_b.workspace_code);
}

#[tokio::test]
async fn create_retrieval_trace_surfaces_stage2_provenance_fields() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = trimmed.split_once('=') {
                unsafe {
                    std::env::set_var(key.trim(), value.trim_matches('\"'));
                }
            }
        }
    }

    let cfg = AppConfig::from_env().expect("config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let (_workspace_code, _source_project_code, target_project_code, _transfer_policy_code) =
        create_stage2_import_shared_context(&client, suffix).await;
    let ids = client
        .query_one(
            r#"
                SELECT p.workspace_id, p.project_id, n.namespace_id
                FROM ami.projects p
                JOIN ami.namespaces n
                  ON n.project_id = p.project_id
                WHERE p.code = $1
                ORDER BY n.created_at ASC
                LIMIT 1
                "#,
            &[&target_project_code],
        )
        .await
        .expect("target ids");
    let workspace_id: Uuid = ids.get(0);
    let project_id: Uuid = ids.get(1);
    let namespace_id: Uuid = ids.get(2);
    let context_pack_id = Uuid::new_v4();
    insert_context_pack(
        &client,
        &ContextPackInsert {
            context_pack_id,
            project_id,
            namespace_id,
            retrieval_mode: "local_strict",
            query_text: "stage2 retrieval trace proof",
            visible_projects: &json!([target_project_code]),
            payload: &json!({"proof":"stage2"}),
            artifact_ref_id: None,
        },
    )
    .await
    .expect("context pack");

    let source_event_ids = json!([format!("context_pack:{context_pack_id}")]);
    let artifact_refs = json!([format!("artifact://proof/retrieval-trace/{suffix}")]);
    let message_refs = json!([format!("message:retrieval-trace:{suffix}")]);
    let evidence_span = json!({
        "kind": "retrieval_trace",
        "context_pack_id": context_pack_id,
        "layer": "structured_graph_neighborhood"
    });
    let retrieval_trace_id = create_retrieval_trace(
            &client,
            &RetrievalTraceInsert {
                workspace_id,
                project_id,
                namespace_id,
                context_pack_id: Some(context_pack_id),
                query_text: "stage2 retrieval trace proof".to_string(),
                requested_mode: Some("local_strict".to_string()),
                effective_mode: Some("local_strict".to_string()),
                scope_filter: json!({"visible_projects":[target_project_code]}),
                candidate_summary: json!({"candidate_generation":{"structured":1}}),
                rerank_summary: json!({"scope_resolver":{"mode":"local_strict"}}),
                evidence_sufficiency: json!({"cheapest_sufficient_layer":"structured_graph_neighborhood"}),
                source_kind: Some("context_pack_retrieval_runtime".to_string()),
                source_event_ids: source_event_ids.clone(),
                artifact_refs: artifact_refs.clone(),
                message_refs: message_refs.clone(),
                evidence_span: evidence_span.clone(),
                derivation_kind: Some("extract".to_string()),
                schema_version: Some("retrieval-trace-envelope-v1".to_string()),
                final_decision: "continue".to_string(),
                temporal_query_epoch_ms: Some(1_234_567),
                trace_payload: json!({"decision_trace":{"proof":"stage2"}}),
            },
        )
        .await
        .expect("retrieval trace");

    let row = client
        .query_one(
            r#"
                SELECT
                    source_kind,
                    source_event_ids,
                    artifact_refs,
                    message_refs,
                    evidence_span,
                    derivation_kind,
                    schema_version
                FROM ami.retrieval_traces
                WHERE retrieval_trace_id = $1
                "#,
            &[&retrieval_trace_id],
        )
        .await
        .expect("retrieval trace row");
    let source_kind: Option<String> = row.get(0);
    assert_eq!(
        source_kind.as_deref(),
        Some("context_pack_retrieval_runtime")
    );
    assert_eq!(row.get::<_, Value>(1), source_event_ids);
    assert_eq!(row.get::<_, Value>(2), artifact_refs);
    assert_eq!(row.get::<_, Value>(3), message_refs);
    let stored_evidence_span = row.get::<_, Value>(4);
    assert_eq!(stored_evidence_span["kind"], evidence_span["kind"]);
    assert_eq!(
        stored_evidence_span["stage2_runtime"]["policy_and_scope_filter"]["scope_binding_valid"],
        json!(true)
    );
    assert_eq!(
        stored_evidence_span["stage2_runtime"]["verification_conflict_check"]["write_allowed"],
        json!(true)
    );
    assert_eq!(row.get::<_, String>(5), "extract");
    assert_eq!(row.get::<_, String>(6), "retrieval-trace-envelope-v1");
}

#[tokio::test]
async fn create_retrieval_trace_policy_scope_filter_rejects_context_pack_scope_mismatch() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = trimmed.split_once('=') {
                unsafe {
                    std::env::set_var(key.trim(), value.trim_matches('\"'));
                }
            }
        }
    }

    let cfg = AppConfig::from_env().expect("config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let (_workspace_code, _source_project_code, target_project_code, _transfer_policy_code) =
        create_stage2_import_shared_context(&client, suffix).await;
    let ids = client
        .query_one(
            r#"
                SELECT p.workspace_id, p.project_id, n.namespace_id
                FROM ami.projects p
                JOIN ami.namespaces n
                  ON n.project_id = p.project_id
                WHERE p.code = $1
                ORDER BY n.created_at ASC
                LIMIT 1
                "#,
            &[&target_project_code],
        )
        .await
        .expect("target ids");
    let workspace_id: Uuid = ids.get(0);
    let project_id: Uuid = ids.get(1);
    let namespace_id: Uuid = ids.get(2);
    let mismatched_project_code = format!("retrieval_ctx_mismatch_{suffix}");
    let repo_root = format!("/tmp/{mismatched_project_code}");
    std::fs::create_dir_all(&repo_root).expect("repo root");
    let mismatched_project = upsert_project(
        &client,
        &mismatched_project_code,
        "Retrieval Mismatch Project",
        &repo_root,
        Some("main"),
        "default",
        "project_shared",
        "local_strict",
    )
    .await
    .expect("mismatch project");
    let mismatched_namespace = ensure_namespace(
        &client,
        mismatched_project.project_id,
        "review",
        Some("Review"),
        "local_strict",
    )
    .await
    .expect("mismatch namespace");
    let context_pack_id = Uuid::new_v4();
    insert_context_pack(
        &client,
        &ContextPackInsert {
            context_pack_id,
            project_id: mismatched_project.project_id,
            namespace_id: mismatched_namespace.namespace_id,
            retrieval_mode: "local_strict",
            query_text: "stage2 retrieval trace mismatch",
            visible_projects: &json!([mismatched_project_code]),
            payload: &json!({"proof":"stage2-mismatch"}),
            artifact_ref_id: None,
        },
    )
    .await
    .expect("context pack");

    let error = create_retrieval_trace(
            &client,
            &RetrievalTraceInsert {
                workspace_id,
                project_id,
                namespace_id,
                context_pack_id: Some(context_pack_id),
                query_text: "stage2 retrieval trace mismatch".to_string(),
                requested_mode: Some("local_strict".to_string()),
                effective_mode: Some("local_strict".to_string()),
                scope_filter: json!({"visible_projects":[target_project_code]}),
                candidate_summary: json!({"candidate_generation":{"structured":1}}),
                rerank_summary: json!({"scope_resolver":{"mode":"local_strict"}}),
                evidence_sufficiency: json!({"cheapest_sufficient_layer":"structured_graph_neighborhood"}),
                source_kind: Some("context_pack_retrieval_runtime".to_string()),
                source_event_ids: json!([format!("context_pack:{context_pack_id}")]),
                artifact_refs: json!([format!("artifact://proof/retrieval-trace-mismatch/{suffix}")]),
                message_refs: json!([format!("message:retrieval-trace-mismatch:{suffix}")]),
                evidence_span: json!({"kind":"retrieval_trace","case":"context-pack-scope-mismatch"}),
                derivation_kind: Some("extract".to_string()),
                schema_version: Some("retrieval-trace-envelope-v1".to_string()),
                final_decision: "continue".to_string(),
                temporal_query_epoch_ms: Some(1_234_567),
                trace_payload: json!({"decision_trace":{"cheapest_sufficient_layer":"structured_graph_neighborhood"}}),
            },
        )
        .await
        .expect_err("mismatched context pack rejected");
    assert!(error.to_string().contains("does not match target"));
}

#[tokio::test]
async fn create_retrieval_trace_verification_conflict_check_detects_decision_trace_mismatch() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = trimmed.split_once('=') {
                unsafe {
                    std::env::set_var(key.trim(), value.trim_matches('\"'));
                }
            }
        }
    }

    let cfg = AppConfig::from_env().expect("config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let (_workspace_code, _source_project_code, target_project_code, _transfer_policy_code) =
        create_stage2_import_shared_context(&client, suffix).await;
    let ids = client
        .query_one(
            r#"
                SELECT p.workspace_id, p.project_id, n.namespace_id
                FROM ami.projects p
                JOIN ami.namespaces n
                  ON n.project_id = p.project_id
                WHERE p.code = $1
                ORDER BY n.created_at ASC
                LIMIT 1
                "#,
            &[&target_project_code],
        )
        .await
        .expect("target ids");
    let workspace_id: Uuid = ids.get(0);
    let project_id: Uuid = ids.get(1);
    let namespace_id: Uuid = ids.get(2);
    let context_pack_id = Uuid::new_v4();
    insert_context_pack(
        &client,
        &ContextPackInsert {
            context_pack_id,
            project_id,
            namespace_id,
            retrieval_mode: "local_strict",
            query_text: "stage2 retrieval trace decision mismatch",
            visible_projects: &json!([target_project_code]),
            payload: &json!({"proof":"stage2-decision-mismatch"}),
            artifact_ref_id: None,
        },
    )
    .await
    .expect("context pack");

    let error = create_retrieval_trace(
            &client,
            &RetrievalTraceInsert {
                workspace_id,
                project_id,
                namespace_id,
                context_pack_id: Some(context_pack_id),
                query_text: "stage2 retrieval trace decision mismatch".to_string(),
                requested_mode: Some("local_strict".to_string()),
                effective_mode: Some("local_strict".to_string()),
                scope_filter: json!({"visible_projects":[target_project_code]}),
                candidate_summary: json!({"candidate_generation":{"structured":1}}),
                rerank_summary: json!({"scope_resolver":{"mode":"local_strict"}}),
                evidence_sufficiency: json!({"cheapest_sufficient_layer":"structured_graph_neighborhood"}),
                source_kind: Some("context_pack_retrieval_runtime".to_string()),
                source_event_ids: json!([format!("context_pack:{context_pack_id}")]),
                artifact_refs: json!([format!("artifact://proof/retrieval-trace-decision-mismatch/{suffix}")]),
                message_refs: json!([format!("message:retrieval-trace-decision-mismatch:{suffix}")]),
                evidence_span: json!({"kind":"retrieval_trace","case":"decision-trace-mismatch"}),
                derivation_kind: Some("extract".to_string()),
                schema_version: Some("retrieval-trace-envelope-v1".to_string()),
                final_decision: "continue".to_string(),
                temporal_query_epoch_ms: Some(1_234_567),
                trace_payload: json!({"decision_trace":{"cheapest_sufficient_layer":"summary"}}),
            },
        )
        .await
        .expect_err("decision trace mismatch rejected");
    assert!(
        error
            .to_string()
            .contains("cheapest sufficient layer disagrees")
    );
}

#[tokio::test]
async fn create_memory_provenance_surfaces_stage2_runtime_fields() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = trimmed.split_once('=') {
                unsafe {
                    std::env::set_var(key.trim(), value.trim_matches('\"'));
                }
            }
        }
    }

    let cfg = AppConfig::from_env().expect("config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let (_workspace_code, _source_project_code, target_project_code, _transfer_policy_code) =
        create_stage2_import_shared_context(&client, suffix).await;
    let ids = client
        .query_one(
            r#"
                SELECT p.project_id, n.namespace_id
                FROM ami.projects p
                JOIN ami.namespaces n ON n.project_id = p.project_id
                WHERE p.code = $1 AND n.code = 'default'
                "#,
            &[&target_project_code],
        )
        .await
        .expect("target ids");
    let project_id: Uuid = ids.get(0);
    let namespace_id: Uuid = ids.get(1);

    let memory_item = create_memory_item(
        &client,
        &target_project_code,
        "default",
        &MemoryItemInsert {
            source_project_code: None,
            import_packet_id: None,
            owner_agent_code: None,
            item_kind: "fact",
            sensitivity_class: Some("internal"),
            title: "memory provenance proof item",
            summary: Some("memory provenance proof summary"),
            body: None,
            identity_key: Some(&format!("memory-provenance-{suffix}")),
            truth_state: Some("proposed"),
            trust_state: Some("proposed"),
            verification_state: Some("unverified"),
            lifecycle_state: Some("hot"),
            source_event_ids: &[format!("event:memory-provenance-item:{suffix}")],
            artifact_refs: &[format!("artifact://proof/memory-provenance-item/{suffix}")],
            message_refs: &[format!("message:memory-provenance-item:{suffix}")],
            evidence_span: &json!({"kind":"memory_item","surface":"memory-provenance-test"}),
            derivation_kind: Some("extract"),
            observed_at_epoch_ms: Some(1_000),
            recorded_at_epoch_ms: Some(1_005),
            valid_from_epoch_ms: Some(1_000),
            valid_to_epoch_ms: Some(2_000),
            last_verified_at_epoch_ms: None,
            object_version: Some(1),
            causation_id: None,
            correlation_id: None,
            utility_score: None,
            retention_class: None,
            freshness_score: None,
            ttl_epoch_ms: None,
            decay_policy: None,
            consolidation_status: None,
            imported_from: None,
            schema_version: Some("memory-item-envelope-v1"),
            superseded_by_memory_item_id: None,
            metadata: &json!({}),
        },
    )
    .await
    .expect("memory item");

    let artifact_ref = create_artifact_ref(
        &client,
        &target_project_code,
        "default",
        &ArtifactRefInsert {
            project_id,
            namespace_id,
            artifact_kind: "log_excerpt",
            bucket: "proof-bucket",
            object_key: &format!("proof/memory-provenance/{suffix}.json"),
            content_type: Some("application/json"),
            source_kind: Some("proof_contract"),
            source_event_ids: Some(&json!([format!(
                "event:memory-provenance-artifact:{suffix}"
            )])),
            message_refs: Some(&json!([format!(
                "message:memory-provenance-artifact:{suffix}"
            )])),
            evidence_span: Some(&json!({"kind":"artifact_ref","surface":"memory-provenance-test"})),
            derivation_kind: Some("extract"),
            schema_version: Some("artifact-ref-envelope-v1"),
            metadata: &json!({"proof":"stage2"}),
        },
    )
    .await
    .expect("artifact ref");

    let snapshot_payload = json!({
        "working_state_restore": {
            "project": {"code": target_project_code},
            "namespace": {"code": "default"},
            "state_lineage": {
                "authoritative_event_id": format!("event:memory-provenance-snapshot:{suffix}"),
                "authoritative_event_kind": "continuity_handoff"
            }
        }
    });
    let source_snapshot_id =
        insert_observability_snapshot(&client, "working_state_restore", &snapshot_payload)
            .await
            .expect("snapshot");

    let provenance = create_memory_provenance(
        &client,
        &target_project_code,
        "default",
        &MemoryProvenanceInsert {
            memory_item_id: Some(memory_item.memory_item_id),
            source_kind: "proof_contract",
            source_event_id: Some(&format!("event:memory-provenance:{suffix}")),
            source_snapshot_id: Some(source_snapshot_id),
            artifact_ref_id: Some(artifact_ref.artifact_ref_id),
            trust_level: Some("verified"),
            message_refs: Some(&json!([format!("message:memory-provenance:{suffix}")])),
            evidence_span: Some(&json!({"source":"proof","range":"1-3"})),
            derivation_kind: Some("extract"),
            observed_at_epoch_ms: Some(1_000),
            recorded_at_epoch_ms: Some(1_005),
            valid_from_epoch_ms: Some(1_000),
            valid_to_epoch_ms: Some(2_000),
            schema_version: Some("memory-provenance-v1"),
            details: &json!({"proof":"stage2"}),
        },
    )
    .await
    .expect("memory provenance");

    assert_eq!(provenance.source_kind, "proof_contract");
    assert_eq!(provenance.trust_level, "verified");
    assert_eq!(provenance.evidence_span["source"], json!("proof"));
    assert_eq!(
        provenance.evidence_span["stage2_runtime"]["policy_and_scope_filter"]["scope_binding_valid"],
        json!(true)
    );
    assert_eq!(
        provenance.evidence_span["stage2_runtime"]["verification_conflict_check"]["write_allowed"],
        json!(true)
    );
}

#[tokio::test]
async fn create_memory_provenance_policy_scope_filter_rejects_memory_item_scope_mismatch() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = trimmed.split_once('=') {
                unsafe {
                    std::env::set_var(key.trim(), value.trim_matches('\"'));
                }
            }
        }
    }

    let cfg = AppConfig::from_env().expect("config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let (_workspace_code, _source_project_code, target_project_code, _transfer_policy_code) =
        create_stage2_import_shared_context(&client, suffix).await;
    let other_project_code = format!("memory_provenance_other_proj_{suffix}");
    let repo_root = format!("/tmp/{other_project_code}");
    std::fs::create_dir_all(&repo_root).expect("repo root");
    let other_project = upsert_project(
        &client,
        &other_project_code,
        "Memory Provenance Other Project",
        &repo_root,
        Some("main"),
        "default",
        "project_shared",
        "local_strict",
    )
    .await
    .expect("other project");
    ensure_namespace(
        &client,
        other_project.project_id,
        "default",
        Some("Default"),
        "local_strict",
    )
    .await
    .expect("other namespace");
    let other_item = create_memory_item(
        &client,
        &other_project_code,
        "default",
        &MemoryItemInsert {
            source_project_code: None,
            import_packet_id: None,
            owner_agent_code: None,
            item_kind: "fact",
            sensitivity_class: Some("internal"),
            title: "other item",
            summary: Some("other item"),
            body: None,
            identity_key: Some(&format!("other-item-{suffix}")),
            truth_state: Some("proposed"),
            trust_state: Some("proposed"),
            verification_state: Some("unverified"),
            lifecycle_state: Some("hot"),
            source_event_ids: &[format!("event:other-item:{suffix}")],
            artifact_refs: &[format!("artifact://proof/other-item/{suffix}")],
            message_refs: &[format!("message:other-item:{suffix}")],
            evidence_span: &json!({"kind":"memory_item","surface":"other"}),
            derivation_kind: Some("extract"),
            observed_at_epoch_ms: Some(1_000),
            recorded_at_epoch_ms: Some(1_005),
            valid_from_epoch_ms: Some(1_000),
            valid_to_epoch_ms: Some(2_000),
            last_verified_at_epoch_ms: None,
            object_version: Some(1),
            causation_id: None,
            correlation_id: None,
            utility_score: None,
            retention_class: None,
            freshness_score: None,
            ttl_epoch_ms: None,
            decay_policy: None,
            consolidation_status: None,
            imported_from: None,
            schema_version: Some("memory-item-envelope-v1"),
            superseded_by_memory_item_id: None,
            metadata: &json!({}),
        },
    )
    .await
    .expect("other item");

    let error = create_memory_provenance(
        &client,
        &target_project_code,
        "default",
        &MemoryProvenanceInsert {
            memory_item_id: Some(other_item.memory_item_id),
            source_kind: "proof_contract",
            source_event_id: Some(&format!("event:memory-provenance-mismatch:{suffix}")),
            source_snapshot_id: None,
            artifact_ref_id: None,
            trust_level: Some("verified"),
            message_refs: Some(&json!([format!(
                "message:memory-provenance-mismatch:{suffix}"
            )])),
            evidence_span: Some(&json!({"source":"proof","case":"memory-item-scope-mismatch"})),
            derivation_kind: Some("extract"),
            observed_at_epoch_ms: Some(1_000),
            recorded_at_epoch_ms: Some(1_005),
            valid_from_epoch_ms: Some(1_000),
            valid_to_epoch_ms: Some(2_000),
            schema_version: Some("memory-provenance-v1"),
            details: &json!({"proof":"stage2"}),
        },
    )
    .await
    .expect_err("memory item scope mismatch rejected");
    assert!(
        error
            .to_string()
            .contains("memory item scope does not match")
    );
}

#[tokio::test]
async fn create_memory_provenance_verification_conflict_check_detects_poisoned_evidence_span() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = trimmed.split_once('=') {
                unsafe {
                    std::env::set_var(key.trim(), value.trim_matches('\"'));
                }
            }
        }
    }

    let cfg = AppConfig::from_env().expect("config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let (_workspace_code, _source_project_code, target_project_code, _transfer_policy_code) =
        create_stage2_import_shared_context(&client, suffix).await;

    let error = create_memory_provenance(
        &client,
        &target_project_code,
        "default",
        &MemoryProvenanceInsert {
            memory_item_id: None,
            source_kind: "proof_contract",
            source_event_id: Some(&format!("event:memory-provenance-poison:{suffix}")),
            source_snapshot_id: None,
            artifact_ref_id: None,
            trust_level: Some("verified"),
            message_refs: Some(&json!([format!(
                "message:memory-provenance-poison:{suffix}"
            )])),
            evidence_span: Some(&json!({"source":"proof","poisoned":true})),
            derivation_kind: Some("extract"),
            observed_at_epoch_ms: Some(1_000),
            recorded_at_epoch_ms: Some(1_005),
            valid_from_epoch_ms: Some(1_000),
            valid_to_epoch_ms: Some(2_000),
            schema_version: Some("memory-provenance-v1"),
            details: &json!({"proof":"stage2"}),
        },
    )
    .await
    .expect_err("poisoned memory provenance rejected");
    assert!(error.to_string().contains("flagged poisoned"));
}

#[tokio::test]
async fn create_restore_pack_surfaces_stage2_provenance_fields() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = trimmed.split_once('=') {
                unsafe {
                    std::env::set_var(key.trim(), value.trim_matches('\"'));
                }
            }
        }
    }

    let cfg = AppConfig::from_env().expect("config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let (_workspace_code, _source_project_code, target_project_code, _transfer_policy_code) =
        create_stage2_import_shared_context(&client, suffix).await;
    let namespace_code = "default";
    let snapshot_payload = json!({
        "working_state_restore": {
            "project": {"code": target_project_code},
            "namespace": {"code": namespace_code},
            "captured_at_epoch_ms": 1_234_567,
            "state_lineage": {
                "authoritative_event_id": format!("event:restore-pack:{suffix}"),
                "authoritative_event_kind": "continuity_handoff"
            }
        }
    });
    let source_snapshot_id =
        insert_observability_snapshot(&client, "working_state_restore", &snapshot_payload)
            .await
            .expect("restore snapshot");
    let source_event_ids = json!([format!("event:restore-pack:{suffix}")]);
    let artifact_refs = json!([format!("artifact://proof/restore-pack/{suffix}")]);
    let message_refs = json!([format!("thread:restore-pack:{suffix}")]);
    let evidence_span = json!({
        "kind": "working_state_restore",
        "authoritative_event_id": format!("event:restore-pack:{suffix}"),
        "restore_confidence": "durable"
    });
    let payload = json!({
        "project": {"code": target_project_code},
        "namespace": {"code": namespace_code},
        "current_goal": format!("restore pack goal {suffix}"),
        "next_step": "continue restore proof",
        "recent_actions": [{"event_id": format!("event:restore-pack:{suffix}")}]
    });
    let restore_pack = create_restore_pack(
        &client,
        &target_project_code,
        namespace_code,
        &RestorePackInsert {
            agent_scope: Some("proof::restore"),
            session_id: Some("session-restore-pack"),
            thread_id: Some("thread-restore-pack"),
            source_snapshot_id: Some(source_snapshot_id),
            source_snapshot_hint: None,
            pack_kind: "workspace_restore_pack",
            source_kind: Some("working_state_restore_runtime"),
            source_event_ids: Some(&source_event_ids),
            artifact_refs: Some(&artifact_refs),
            message_refs: Some(&message_refs),
            evidence_span: Some(&evidence_span),
            derivation_kind: Some("summary"),
            schema_version: Some("restore-pack-envelope-v1"),
            headline: Some("restore pack headline"),
            summary: Some("restore pack summary"),
            payload: &payload,
            captured_at_epoch_ms: Some(1_234_567),
        },
    )
    .await
    .expect("restore pack");

    assert_eq!(
        restore_pack.source_kind.as_deref(),
        Some("working_state_restore_runtime")
    );
    assert_eq!(restore_pack.source_event_ids, source_event_ids);
    assert_eq!(restore_pack.artifact_refs, artifact_refs);
    assert_eq!(restore_pack.message_refs, message_refs);
    assert_eq!(restore_pack.evidence_span["kind"], evidence_span["kind"]);
    assert_eq!(
        restore_pack.evidence_span["stage2_runtime"]["policy_and_scope_filter"]["scope_binding_valid"],
        json!(true)
    );
    assert_eq!(
        restore_pack.evidence_span["stage2_runtime"]["policy_and_scope_filter"]["snapshot_kind_valid"],
        json!(true)
    );
    assert_eq!(
        restore_pack.evidence_span["stage2_runtime"]["verification_conflict_check"]["write_allowed"],
        json!(true)
    );
    assert_eq!(restore_pack.derivation_kind, "summary");
    assert_eq!(restore_pack.schema_version, "restore-pack-envelope-v1");
    assert_eq!(restore_pack.pack_kind, "workspace_restore_pack");
    assert_eq!(restore_pack.source_snapshot_id, Some(source_snapshot_id));
}

#[tokio::test]
async fn create_restore_pack_policy_scope_filter_requires_source_snapshot_for_workspace_restore_pack()
 {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = trimmed.split_once('=') {
                unsafe {
                    std::env::set_var(key.trim(), value.trim_matches('\"'));
                }
            }
        }
    }

    let cfg = AppConfig::from_env().expect("config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let (_workspace_code, _source_project_code, target_project_code, _transfer_policy_code) =
        create_stage2_import_shared_context(&client, suffix).await;
    let error = create_restore_pack(
        &client,
        &target_project_code,
        "default",
        &RestorePackInsert {
            agent_scope: Some("proof::restore"),
            session_id: Some("session-restore-pack-missing-snapshot"),
            thread_id: Some("thread-restore-pack-missing-snapshot"),
            source_snapshot_id: None,
            source_snapshot_hint: None,
            pack_kind: "workspace_restore_pack",
            source_kind: Some("working_state_restore_runtime"),
            source_event_ids: Some(&json!([format!("event:restore-pack-missing:{suffix}")])),
            artifact_refs: Some(&json!([format!(
                "artifact://proof/restore-pack-missing/{suffix}"
            )])),
            message_refs: Some(&json!([format!("thread:restore-pack-missing:{suffix}")])),
            evidence_span: Some(&json!({"kind":"working_state_restore","case":"missing-snapshot"})),
            derivation_kind: Some("summary"),
            schema_version: Some("restore-pack-envelope-v1"),
            headline: Some("restore pack missing snapshot"),
            summary: Some("restore pack missing snapshot"),
            payload: &json!({
                "project": {"code": target_project_code},
                "namespace": {"code": "default"},
                "current_goal": "missing snapshot"
            }),
            captured_at_epoch_ms: Some(1_234_567),
        },
    )
    .await
    .expect_err("workspace restore pack without snapshot rejected");
    assert!(error.to_string().contains("requires source_snapshot_id"));
}

#[tokio::test]
async fn create_restore_pack_policy_scope_filter_rejects_snapshot_scope_mismatch() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = trimmed.split_once('=') {
                unsafe {
                    std::env::set_var(key.trim(), value.trim_matches('\"'));
                }
            }
        }
    }

    let cfg = AppConfig::from_env().expect("config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let (_workspace_code, _source_project_code, target_project_code, _transfer_policy_code) =
        create_stage2_import_shared_context(&client, suffix).await;
    let snapshot_payload = json!({
        "working_state_restore": {
            "project": {"code": format!("other-project-{suffix}")},
            "namespace": {"code": "default"},
            "captured_at_epoch_ms": 1_234_567,
            "state_lineage": {
                "authoritative_event_id": format!("event:restore-pack-mismatch:{suffix}"),
                "authoritative_event_kind": "continuity_handoff"
            }
        }
    });
    let source_snapshot_id =
        insert_observability_snapshot(&client, "working_state_restore", &snapshot_payload)
            .await
            .expect("restore snapshot mismatch");
    let error = create_restore_pack(
        &client,
        &target_project_code,
        "default",
        &RestorePackInsert {
            agent_scope: Some("proof::restore"),
            session_id: Some("session-restore-pack-mismatch"),
            thread_id: Some("thread-restore-pack-mismatch"),
            source_snapshot_id: Some(source_snapshot_id),
            source_snapshot_hint: None,
            pack_kind: "workspace_restore_pack",
            source_kind: Some("working_state_restore_runtime"),
            source_event_ids: Some(&json!([format!("event:restore-pack-mismatch:{suffix}")])),
            artifact_refs: Some(&json!([format!(
                "artifact://proof/restore-pack-mismatch/{suffix}"
            )])),
            message_refs: Some(&json!([format!("thread:restore-pack-mismatch:{suffix}")])),
            evidence_span: Some(&json!({"kind":"working_state_restore","case":"scope-mismatch"})),
            derivation_kind: Some("summary"),
            schema_version: Some("restore-pack-envelope-v1"),
            headline: Some("restore pack mismatch"),
            summary: Some("restore pack mismatch"),
            payload: &json!({
                "project": {"code": target_project_code},
                "namespace": {"code": "default"},
                "current_goal": "scope mismatch"
            }),
            captured_at_epoch_ms: Some(1_234_567),
        },
    )
    .await
    .expect_err("mismatched restore snapshot rejected");
    assert!(error.to_string().contains("does not match target"));
}

#[tokio::test]
async fn create_restore_pack_verification_conflict_check_detects_poisoned_evidence_span() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = trimmed.split_once('=') {
                unsafe {
                    std::env::set_var(key.trim(), value.trim_matches('\"'));
                }
            }
        }
    }

    let cfg = AppConfig::from_env().expect("config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let (_workspace_code, _source_project_code, target_project_code, _transfer_policy_code) =
        create_stage2_import_shared_context(&client, suffix).await;
    let snapshot_payload = json!({
        "working_state_restore": {
            "project": {"code": target_project_code},
            "namespace": {"code": "default"},
            "captured_at_epoch_ms": 1_234_567,
            "state_lineage": {
                "authoritative_event_id": format!("event:restore-pack-poison:{suffix}"),
                "authoritative_event_kind": "continuity_handoff"
            }
        }
    });
    let source_snapshot_id =
        insert_observability_snapshot(&client, "working_state_restore", &snapshot_payload)
            .await
            .expect("restore snapshot poison");
    let error = create_restore_pack(
        &client,
        &target_project_code,
        "default",
        &RestorePackInsert {
            agent_scope: Some("proof::restore"),
            session_id: Some("session-restore-pack-poison"),
            thread_id: Some("thread-restore-pack-poison"),
            source_snapshot_id: Some(source_snapshot_id),
            source_snapshot_hint: None,
            pack_kind: "workspace_restore_pack",
            source_kind: Some("working_state_restore_runtime"),
            source_event_ids: Some(&json!([format!("event:restore-pack-poison:{suffix}")])),
            artifact_refs: Some(&json!([format!(
                "artifact://proof/restore-pack-poison/{suffix}"
            )])),
            message_refs: Some(&json!([format!("thread:restore-pack-poison:{suffix}")])),
            evidence_span: Some(&json!({"kind":"working_state_restore","poisoned":true})),
            derivation_kind: Some("summary"),
            schema_version: Some("restore-pack-envelope-v1"),
            headline: Some("restore pack poison"),
            summary: Some("restore pack poison"),
            payload: &json!({
                "project": {"code": target_project_code},
                "namespace": {"code": "default"},
                "current_goal": "poisoned"
            }),
            captured_at_epoch_ms: Some(1_234_567),
        },
    )
    .await
    .expect_err("poisoned restore pack rejected");
    assert!(error.to_string().contains("flagged poisoned"));
}

#[tokio::test]
async fn create_memory_edge_surfaces_stage2_provenance_fields() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = trimmed.split_once('=') {
                unsafe {
                    std::env::set_var(key.trim(), value.trim_matches('\"'));
                }
            }
        }
    }

    let cfg = AppConfig::from_env().expect("config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let (_workspace_code, _source_project_code, target_project_code, _transfer_policy_code) =
        create_stage2_import_shared_context(&client, suffix).await;
    let namespace_code = "default";
    let source_event_ids = vec![format!("event:memory-edge:{suffix}")];
    let artifact_refs = vec![format!("artifact://proof/memory-edge/{suffix}")];
    let message_refs = vec![format!("thread:memory-edge:{suffix}")];
    let source_event_ids_json = json!(source_event_ids);
    let artifact_refs_json = json!(artifact_refs);
    let message_refs_json = json!(message_refs);
    let evidence_span = json!({
        "kind": "memory_edge_proof",
        "event_id": format!("event:memory-edge:{suffix}")
    });
    let left = create_memory_item(
        &client,
        &target_project_code,
        namespace_code,
        &MemoryItemInsert {
            source_project_code: None,
            import_packet_id: None,
            item_kind: "fact",
            owner_agent_code: Some("stage2-proof"),
            identity_key: Some(&format!("memory-edge-left-{suffix}")),
            title: "memory edge left",
            summary: Some("memory edge left summary"),
            body: Some("memory edge left body"),
            sensitivity_class: Some("internal"),
            truth_state: Some("current"),
            trust_state: Some("verified"),
            verification_state: Some("verified"),
            lifecycle_state: Some("hot"),
            source_event_ids: &source_event_ids,
            artifact_refs: &artifact_refs,
            message_refs: &message_refs,
            evidence_span: &evidence_span,
            derivation_kind: Some("extract"),
            observed_at_epoch_ms: Some(1_001),
            recorded_at_epoch_ms: Some(1_001),
            valid_from_epoch_ms: Some(1_001),
            valid_to_epoch_ms: None,
            last_verified_at_epoch_ms: Some(1_001),
            object_version: None,
            utility_score: Some(0.7),
            freshness_score: Some(0.8),
            retention_class: Some("standard"),
            ttl_epoch_ms: None,
            decay_policy: None,
            consolidation_status: None,
            imported_from: None,
            schema_version: Some("memory-envelope-v1"),
            superseded_by_memory_item_id: None,
            metadata: &json!({}),
            causation_id: None,
            correlation_id: None,
        },
    )
    .await
    .expect("left memory item");
    let right = create_memory_item(
        &client,
        &target_project_code,
        namespace_code,
        &MemoryItemInsert {
            source_project_code: None,
            import_packet_id: None,
            item_kind: "fact",
            owner_agent_code: Some("stage2-proof"),
            identity_key: Some(&format!("memory-edge-right-{suffix}")),
            title: "memory edge right",
            summary: Some("memory edge right summary"),
            body: Some("memory edge right body"),
            sensitivity_class: Some("internal"),
            truth_state: Some("current"),
            trust_state: Some("verified"),
            verification_state: Some("verified"),
            lifecycle_state: Some("hot"),
            source_event_ids: &source_event_ids,
            artifact_refs: &artifact_refs,
            message_refs: &message_refs,
            evidence_span: &evidence_span,
            derivation_kind: Some("extract"),
            observed_at_epoch_ms: Some(1_002),
            recorded_at_epoch_ms: Some(1_002),
            valid_from_epoch_ms: Some(1_002),
            valid_to_epoch_ms: None,
            last_verified_at_epoch_ms: Some(1_002),
            object_version: None,
            utility_score: Some(0.7),
            freshness_score: Some(0.8),
            retention_class: Some("standard"),
            ttl_epoch_ms: None,
            decay_policy: None,
            consolidation_status: None,
            imported_from: None,
            schema_version: Some("memory-envelope-v1"),
            superseded_by_memory_item_id: None,
            metadata: &json!({}),
            causation_id: None,
            correlation_id: None,
        },
    )
    .await
    .expect("right memory item");

    let edge = create_memory_edge(
        &client,
        &target_project_code,
        namespace_code,
        &MemoryEdgeInsert {
            source_memory_item_id: left.memory_item_id,
            target_memory_item_id: right.memory_item_id,
            edge_kind: "supports",
            edge_state: Some("active"),
            trust_state: Some("verified"),
            validity_basis: Some("explicit"),
            score: Some(0.91),
            evidence: &json!({"proof":"stage2-memory-edge"}),
            source_kind: Some("memory_conflict_runtime"),
            source_event_ids: Some(&source_event_ids_json),
            artifact_refs: Some(&artifact_refs_json),
            message_refs: Some(&message_refs_json),
            evidence_span: Some(&evidence_span),
            derivation_kind: Some("extract"),
            schema_version: Some("memory-edge-envelope-v1"),
            valid_from_epoch_ms: Some(1_100),
            valid_to_epoch_ms: None,
        },
    )
    .await
    .expect("memory edge");
    assert_eq!(edge.project_code, target_project_code);
    assert_eq!(edge.namespace_code.as_deref(), Some(namespace_code));
    assert_eq!(edge.edge_kind, "supports");
    assert_eq!(edge.edge_state, "active");
    assert_eq!(edge.trust_state, "verified");
    assert_eq!(edge.validity_basis, "explicit");
    assert_eq!(edge.source_kind.as_deref(), Some("memory_conflict_runtime"));
    assert_eq!(edge.source_event_ids, json!(source_event_ids));
    assert_eq!(edge.artifact_refs, json!(artifact_refs));
    assert_eq!(edge.message_refs, json!(message_refs));
    assert_eq!(edge.evidence_span["kind"], evidence_span["kind"]);
    assert_eq!(
        edge.evidence_span["stage2_runtime"]["policy_and_scope_filter"]["scope_binding_valid"],
        json!(true)
    );
    assert_eq!(
        edge.evidence_span["stage2_runtime"]["verification_conflict_check"]["write_allowed"],
        json!(true)
    );
    assert_eq!(edge.derivation_kind, "extract");
    assert_eq!(edge.schema_version, "memory-edge-envelope-v1");
    assert_eq!(edge.valid_from_epoch_ms, Some(1_100));
}

#[tokio::test]
async fn create_memory_edge_policy_scope_filter_rejects_source_scope_mismatch() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = trimmed.split_once('=') {
                unsafe {
                    std::env::set_var(key.trim(), value.trim_matches('\"'));
                }
            }
        }
    }

    let cfg = AppConfig::from_env().expect("config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let (_workspace_code, _source_project_code, target_project_code, _transfer_policy_code) =
        create_stage2_import_shared_context(&client, suffix).await;
    let other_project_code = format!("memory_edge_other_proj_{suffix}");
    let repo_root = format!("/tmp/{other_project_code}");
    std::fs::create_dir_all(&repo_root).expect("repo root");
    let other_project = upsert_project(
        &client,
        &other_project_code,
        "Memory Edge Other Project",
        &repo_root,
        Some("main"),
        "default",
        "project_shared",
        "local_strict",
    )
    .await
    .expect("other project");
    ensure_namespace(
        &client,
        other_project.project_id,
        "default",
        Some("Default"),
        "local_strict",
    )
    .await
    .expect("other namespace");
    let shared_evidence = json!({"kind":"memory_edge_scope_mismatch"});
    let left = create_memory_item(
        &client,
        &other_project_code,
        "default",
        &MemoryItemInsert {
            source_project_code: None,
            import_packet_id: None,
            item_kind: "fact",
            owner_agent_code: Some("stage2-proof"),
            identity_key: Some(&format!("memory-edge-other-left-{suffix}")),
            title: "other left",
            summary: Some("other left"),
            body: None,
            sensitivity_class: Some("internal"),
            truth_state: Some("current"),
            trust_state: Some("verified"),
            verification_state: Some("verified"),
            lifecycle_state: Some("hot"),
            source_event_ids: &[format!("event:memory-edge-other-left:{suffix}")],
            artifact_refs: &[format!("artifact://proof/memory-edge-other-left/{suffix}")],
            message_refs: &[format!("message:memory-edge-other-left:{suffix}")],
            evidence_span: &shared_evidence,
            derivation_kind: Some("extract"),
            observed_at_epoch_ms: Some(1_001),
            recorded_at_epoch_ms: Some(1_001),
            valid_from_epoch_ms: Some(1_001),
            valid_to_epoch_ms: None,
            last_verified_at_epoch_ms: Some(1_001),
            object_version: None,
            utility_score: Some(0.7),
            freshness_score: Some(0.8),
            retention_class: Some("standard"),
            ttl_epoch_ms: None,
            decay_policy: None,
            consolidation_status: None,
            imported_from: None,
            schema_version: Some("memory-envelope-v1"),
            superseded_by_memory_item_id: None,
            metadata: &json!({}),
            causation_id: None,
            correlation_id: None,
        },
    )
    .await
    .expect("other left item");
    let right = create_memory_item(
        &client,
        &target_project_code,
        "default",
        &MemoryItemInsert {
            source_project_code: None,
            import_packet_id: None,
            item_kind: "fact",
            owner_agent_code: Some("stage2-proof"),
            identity_key: Some(&format!("memory-edge-target-right-{suffix}")),
            title: "target right",
            summary: Some("target right"),
            body: None,
            sensitivity_class: Some("internal"),
            truth_state: Some("current"),
            trust_state: Some("verified"),
            verification_state: Some("verified"),
            lifecycle_state: Some("hot"),
            source_event_ids: &[format!("event:memory-edge-target-right:{suffix}")],
            artifact_refs: &[format!(
                "artifact://proof/memory-edge-target-right/{suffix}"
            )],
            message_refs: &[format!("message:memory-edge-target-right:{suffix}")],
            evidence_span: &shared_evidence,
            derivation_kind: Some("extract"),
            observed_at_epoch_ms: Some(1_002),
            recorded_at_epoch_ms: Some(1_002),
            valid_from_epoch_ms: Some(1_002),
            valid_to_epoch_ms: None,
            last_verified_at_epoch_ms: Some(1_002),
            object_version: None,
            utility_score: Some(0.7),
            freshness_score: Some(0.8),
            retention_class: Some("standard"),
            ttl_epoch_ms: None,
            decay_policy: None,
            consolidation_status: None,
            imported_from: None,
            schema_version: Some("memory-envelope-v1"),
            superseded_by_memory_item_id: None,
            metadata: &json!({}),
            causation_id: None,
            correlation_id: None,
        },
    )
    .await
    .expect("target right item");
    let source_event_ids_json = json!([format!("event:memory-edge-mismatch:{suffix}")]);
    let artifact_refs_json = json!([format!("artifact://proof/memory-edge-mismatch/{suffix}")]);
    let message_refs_json = json!([format!("message:memory-edge-mismatch:{suffix}")]);
    let error = create_memory_edge(
        &client,
        &target_project_code,
        "default",
        &MemoryEdgeInsert {
            source_memory_item_id: left.memory_item_id,
            target_memory_item_id: right.memory_item_id,
            edge_kind: "supports",
            edge_state: Some("active"),
            trust_state: Some("verified"),
            validity_basis: Some("explicit"),
            score: Some(0.5),
            evidence: &json!({"proof":"stage2"}),
            source_kind: Some("runtime_cli"),
            source_event_ids: Some(&source_event_ids_json),
            artifact_refs: Some(&artifact_refs_json),
            message_refs: Some(&message_refs_json),
            evidence_span: Some(&shared_evidence),
            derivation_kind: Some("extract"),
            schema_version: Some("memory-edge-envelope-v1"),
            valid_from_epoch_ms: Some(1_100),
            valid_to_epoch_ms: None,
        },
    )
    .await
    .expect_err("source scope mismatch rejected");
    assert!(
        error
            .to_string()
            .contains("memory edge source memory item scope does not match")
    );
}

#[tokio::test]
async fn create_memory_edge_verification_conflict_check_detects_poisoned_evidence_span() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = trimmed.split_once('=') {
                unsafe {
                    std::env::set_var(key.trim(), value.trim_matches('\"'));
                }
            }
        }
    }

    let cfg = AppConfig::from_env().expect("config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let (_workspace_code, _source_project_code, target_project_code, _transfer_policy_code) =
        create_stage2_import_shared_context(&client, suffix).await;
    let left = create_memory_item(
        &client,
        &target_project_code,
        "default",
        &MemoryItemInsert {
            source_project_code: None,
            import_packet_id: None,
            item_kind: "fact",
            owner_agent_code: Some("stage2-proof"),
            identity_key: Some(&format!("memory-edge-poison-left-{suffix}")),
            title: "left",
            summary: Some("left"),
            body: None,
            sensitivity_class: Some("internal"),
            truth_state: Some("current"),
            trust_state: Some("verified"),
            verification_state: Some("verified"),
            lifecycle_state: Some("hot"),
            source_event_ids: &[format!("event:memory-edge-poison-left:{suffix}")],
            artifact_refs: &[format!("artifact://proof/memory-edge-poison-left/{suffix}")],
            message_refs: &[format!("message:memory-edge-poison-left:{suffix}")],
            evidence_span: &json!({"kind":"memory_edge"}),
            derivation_kind: Some("extract"),
            observed_at_epoch_ms: Some(1_001),
            recorded_at_epoch_ms: Some(1_001),
            valid_from_epoch_ms: Some(1_001),
            valid_to_epoch_ms: None,
            last_verified_at_epoch_ms: Some(1_001),
            object_version: None,
            utility_score: Some(0.7),
            freshness_score: Some(0.8),
            retention_class: Some("standard"),
            ttl_epoch_ms: None,
            decay_policy: None,
            consolidation_status: None,
            imported_from: None,
            schema_version: Some("memory-envelope-v1"),
            superseded_by_memory_item_id: None,
            metadata: &json!({}),
            causation_id: None,
            correlation_id: None,
        },
    )
    .await
    .expect("left item");
    let right = create_memory_item(
        &client,
        &target_project_code,
        "default",
        &MemoryItemInsert {
            source_project_code: None,
            import_packet_id: None,
            item_kind: "fact",
            owner_agent_code: Some("stage2-proof"),
            identity_key: Some(&format!("memory-edge-poison-right-{suffix}")),
            title: "right",
            summary: Some("right"),
            body: None,
            sensitivity_class: Some("internal"),
            truth_state: Some("current"),
            trust_state: Some("verified"),
            verification_state: Some("verified"),
            lifecycle_state: Some("hot"),
            source_event_ids: &[format!("event:memory-edge-poison-right:{suffix}")],
            artifact_refs: &[format!(
                "artifact://proof/memory-edge-poison-right/{suffix}"
            )],
            message_refs: &[format!("message:memory-edge-poison-right:{suffix}")],
            evidence_span: &json!({"kind":"memory_edge"}),
            derivation_kind: Some("extract"),
            observed_at_epoch_ms: Some(1_002),
            recorded_at_epoch_ms: Some(1_002),
            valid_from_epoch_ms: Some(1_002),
            valid_to_epoch_ms: None,
            last_verified_at_epoch_ms: Some(1_002),
            object_version: None,
            utility_score: Some(0.7),
            freshness_score: Some(0.8),
            retention_class: Some("standard"),
            ttl_epoch_ms: None,
            decay_policy: None,
            consolidation_status: None,
            imported_from: None,
            schema_version: Some("memory-envelope-v1"),
            superseded_by_memory_item_id: None,
            metadata: &json!({}),
            causation_id: None,
            correlation_id: None,
        },
    )
    .await
    .expect("right item");
    let error = create_memory_edge(
        &client,
        &target_project_code,
        "default",
        &MemoryEdgeInsert {
            source_memory_item_id: left.memory_item_id,
            target_memory_item_id: right.memory_item_id,
            edge_kind: "supports",
            edge_state: Some("active"),
            trust_state: Some("verified"),
            validity_basis: Some("explicit"),
            score: Some(0.5),
            evidence: &json!({"proof":"stage2"}),
            source_kind: Some("runtime_cli"),
            source_event_ids: Some(&json!([format!("event:memory-edge-poison:{suffix}")])),
            artifact_refs: Some(&json!([format!(
                "artifact://proof/memory-edge-poison/{suffix}"
            )])),
            message_refs: Some(&json!([format!("message:memory-edge-poison:{suffix}")])),
            evidence_span: Some(&json!({"kind":"memory_edge","poisoned":true})),
            derivation_kind: Some("extract"),
            schema_version: Some("memory-edge-envelope-v1"),
            valid_from_epoch_ms: Some(1_100),
            valid_to_epoch_ms: None,
        },
    )
    .await
    .expect_err("poisoned memory edge rejected");
    assert!(
        error
            .to_string()
            .contains("memory edge evidence_span is flagged poisoned")
    );
}

#[tokio::test]
async fn create_memory_conflict_surfaces_stage2_provenance_fields() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = trimmed.split_once('=') {
                unsafe {
                    std::env::set_var(key.trim(), value.trim_matches('\"'));
                }
            }
        }
    }

    let cfg = AppConfig::from_env().expect("config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let (_workspace_code, _source_project_code, target_project_code, _transfer_policy_code) =
        create_stage2_import_shared_context(&client, suffix).await;
    let namespace_code = "default";
    let source_event_ids = vec![format!("event:memory-conflict:{suffix}")];
    let artifact_refs = vec![format!("artifact://proof/memory-conflict/{suffix}")];
    let message_refs = vec![format!("thread:memory-conflict:{suffix}")];
    let source_event_ids_json = json!(source_event_ids);
    let artifact_refs_json = json!(artifact_refs);
    let message_refs_json = json!(message_refs);
    let evidence_span = json!({
        "kind": "memory_conflict_proof",
        "event_id": format!("event:memory-conflict:{suffix}")
    });
    let left = create_memory_item(
        &client,
        &target_project_code,
        namespace_code,
        &MemoryItemInsert {
            source_project_code: None,
            import_packet_id: None,
            item_kind: "fact",
            owner_agent_code: Some("stage2-proof"),
            identity_key: Some(&format!("memory-conflict-left-{suffix}")),
            title: "memory conflict left",
            summary: Some("memory conflict left summary"),
            body: Some("memory conflict left body"),
            sensitivity_class: Some("internal"),
            truth_state: Some("current"),
            trust_state: Some("verified"),
            verification_state: Some("verified"),
            lifecycle_state: Some("hot"),
            source_event_ids: &source_event_ids,
            artifact_refs: &artifact_refs,
            message_refs: &message_refs,
            evidence_span: &evidence_span,
            derivation_kind: Some("extract"),
            observed_at_epoch_ms: Some(2_001),
            recorded_at_epoch_ms: Some(2_001),
            valid_from_epoch_ms: Some(2_001),
            valid_to_epoch_ms: None,
            last_verified_at_epoch_ms: Some(2_001),
            object_version: None,
            utility_score: Some(0.7),
            freshness_score: Some(0.8),
            retention_class: Some("standard"),
            ttl_epoch_ms: None,
            decay_policy: None,
            consolidation_status: None,
            imported_from: None,
            schema_version: Some("memory-envelope-v1"),
            superseded_by_memory_item_id: None,
            metadata: &json!({}),
            causation_id: None,
            correlation_id: None,
        },
    )
    .await
    .expect("left memory item");
    let right = create_memory_item(
        &client,
        &target_project_code,
        namespace_code,
        &MemoryItemInsert {
            source_project_code: None,
            import_packet_id: None,
            item_kind: "fact",
            owner_agent_code: Some("stage2-proof"),
            identity_key: Some(&format!("memory-conflict-right-{suffix}")),
            title: "memory conflict right",
            summary: Some("memory conflict right summary"),
            body: Some("memory conflict right body"),
            sensitivity_class: Some("internal"),
            truth_state: Some("current"),
            trust_state: Some("verified"),
            verification_state: Some("verified"),
            lifecycle_state: Some("hot"),
            source_event_ids: &source_event_ids,
            artifact_refs: &artifact_refs,
            message_refs: &message_refs,
            evidence_span: &evidence_span,
            derivation_kind: Some("extract"),
            observed_at_epoch_ms: Some(2_002),
            recorded_at_epoch_ms: Some(2_002),
            valid_from_epoch_ms: Some(2_002),
            valid_to_epoch_ms: None,
            last_verified_at_epoch_ms: Some(2_002),
            object_version: None,
            utility_score: Some(0.7),
            freshness_score: Some(0.8),
            retention_class: Some("standard"),
            ttl_epoch_ms: None,
            decay_policy: None,
            consolidation_status: None,
            imported_from: None,
            schema_version: Some("memory-envelope-v1"),
            superseded_by_memory_item_id: None,
            metadata: &json!({}),
            causation_id: None,
            correlation_id: None,
        },
    )
    .await
    .expect("right memory item");

    let conflict = create_memory_conflict(
        &client,
        &target_project_code,
        namespace_code,
        &MemoryConflictInsert {
            left_memory_item_id: Some(left.memory_item_id),
            right_memory_item_id: Some(right.memory_item_id),
            conflict_kind: "truth",
            conflict_state: Some("open"),
            severity: Some("high"),
            summary: "truth conflict detected",
            evidence: &json!({"proof":"stage2-memory-conflict"}),
            source_kind: Some("verification_conflict_runtime"),
            source_event_ids: Some(&source_event_ids_json),
            artifact_refs: Some(&artifact_refs_json),
            message_refs: Some(&message_refs_json),
            evidence_span: Some(&evidence_span),
            derivation_kind: Some("extract"),
            schema_version: Some("memory-conflict-envelope-v1"),
            resolution: Some(&json!({})),
            detected_at_epoch_ms: Some(2_100),
            resolved_at_epoch_ms: None,
        },
    )
    .await
    .expect("memory conflict");
    assert_eq!(conflict.project_code, target_project_code);
    assert_eq!(conflict.namespace_code.as_deref(), Some(namespace_code));
    assert_eq!(conflict.conflict_kind, "truth");
    assert_eq!(conflict.conflict_state, "open");
    assert_eq!(conflict.severity, "high");
    assert_eq!(
        conflict.source_kind.as_deref(),
        Some("verification_conflict_runtime")
    );
    assert_eq!(conflict.source_event_ids, json!(source_event_ids));
    assert_eq!(conflict.artifact_refs, json!(artifact_refs));
    assert_eq!(conflict.message_refs, json!(message_refs));
    assert_eq!(conflict.evidence_span["kind"], evidence_span["kind"]);
    assert_eq!(
        conflict.evidence_span["stage2_runtime"]["policy_and_scope_filter"]["scope_binding_valid"],
        json!(true)
    );
    assert_eq!(
        conflict.evidence_span["stage2_runtime"]["verification_conflict_check"]["write_allowed"],
        json!(true)
    );
    assert_eq!(conflict.derivation_kind, "extract");
    assert_eq!(conflict.schema_version, "memory-conflict-envelope-v1");
    assert_eq!(conflict.detected_at_epoch_ms, Some(2_100));

    let edge_row = client
            .query_one(
                r#"
                SELECT edge_kind, edge_state, trust_state, validity_basis, source_kind, source_event_ids, evidence_span
                FROM ami.memory_edges
                WHERE source_memory_item_id = $1
                  AND target_memory_item_id = $2
                  AND edge_kind = 'conflicts_with'
                "#,
                &[&left.memory_item_id, &right.memory_item_id],
            )
            .await
            .expect("conflict edge");
    assert_eq!(edge_row.get::<_, String>(0), "conflicts_with");
    assert_eq!(edge_row.get::<_, String>(1), "active");
    assert_eq!(edge_row.get::<_, String>(2), "disputed");
    assert_eq!(edge_row.get::<_, String>(3), "derived");
    assert_eq!(
        edge_row.get::<_, Option<String>>(4).as_deref(),
        Some("verification_conflict_runtime")
    );
    assert_eq!(edge_row.get::<_, Value>(5), json!(source_event_ids));
    let edge_evidence_span: Value = edge_row.get(6);
    assert_eq!(edge_evidence_span["kind"], evidence_span["kind"]);
    assert_eq!(
        edge_evidence_span["stage2_runtime"]["policy_and_scope_filter"]["scope_binding_valid"],
        json!(true)
    );
}

#[tokio::test]
async fn create_memory_conflict_policy_scope_filter_rejects_left_scope_mismatch() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = trimmed.split_once('=') {
                unsafe {
                    std::env::set_var(key.trim(), value.trim_matches('\"'));
                }
            }
        }
    }

    let cfg = AppConfig::from_env().expect("config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let (_workspace_code, _source_project_code, target_project_code, _transfer_policy_code) =
        create_stage2_import_shared_context(&client, suffix).await;
    let other_project_code = format!("memory_conflict_other_proj_{suffix}");
    let repo_root = format!("/tmp/{other_project_code}");
    std::fs::create_dir_all(&repo_root).expect("repo root");
    let other_project = upsert_project(
        &client,
        &other_project_code,
        "Memory Conflict Other Project",
        &repo_root,
        Some("main"),
        "default",
        "project_shared",
        "local_strict",
    )
    .await
    .expect("other project");
    ensure_namespace(
        &client,
        other_project.project_id,
        "default",
        Some("Default"),
        "local_strict",
    )
    .await
    .expect("other namespace");
    let left = create_memory_item(
        &client,
        &other_project_code,
        "default",
        &MemoryItemInsert {
            source_project_code: None,
            import_packet_id: None,
            item_kind: "fact",
            owner_agent_code: Some("stage2-proof"),
            identity_key: Some(&format!("memory-conflict-other-left-{suffix}")),
            title: "other left",
            summary: Some("other left"),
            body: None,
            sensitivity_class: Some("internal"),
            truth_state: Some("current"),
            trust_state: Some("verified"),
            verification_state: Some("verified"),
            lifecycle_state: Some("hot"),
            source_event_ids: &[format!("event:memory-conflict-other-left:{suffix}")],
            artifact_refs: &[format!(
                "artifact://proof/memory-conflict-other-left/{suffix}"
            )],
            message_refs: &[format!("message:memory-conflict-other-left:{suffix}")],
            evidence_span: &json!({"kind":"memory_conflict"}),
            derivation_kind: Some("extract"),
            observed_at_epoch_ms: Some(2_001),
            recorded_at_epoch_ms: Some(2_001),
            valid_from_epoch_ms: Some(2_001),
            valid_to_epoch_ms: None,
            last_verified_at_epoch_ms: Some(2_001),
            object_version: None,
            utility_score: Some(0.7),
            freshness_score: Some(0.8),
            retention_class: Some("standard"),
            ttl_epoch_ms: None,
            decay_policy: None,
            consolidation_status: None,
            imported_from: None,
            schema_version: Some("memory-envelope-v1"),
            superseded_by_memory_item_id: None,
            metadata: &json!({}),
            causation_id: None,
            correlation_id: None,
        },
    )
    .await
    .expect("other left item");
    let right = create_memory_item(
        &client,
        &target_project_code,
        "default",
        &MemoryItemInsert {
            source_project_code: None,
            import_packet_id: None,
            item_kind: "fact",
            owner_agent_code: Some("stage2-proof"),
            identity_key: Some(&format!("memory-conflict-target-right-{suffix}")),
            title: "target right",
            summary: Some("target right"),
            body: None,
            sensitivity_class: Some("internal"),
            truth_state: Some("current"),
            trust_state: Some("verified"),
            verification_state: Some("verified"),
            lifecycle_state: Some("hot"),
            source_event_ids: &[format!("event:memory-conflict-target-right:{suffix}")],
            artifact_refs: &[format!(
                "artifact://proof/memory-conflict-target-right/{suffix}"
            )],
            message_refs: &[format!("message:memory-conflict-target-right:{suffix}")],
            evidence_span: &json!({"kind":"memory_conflict"}),
            derivation_kind: Some("extract"),
            observed_at_epoch_ms: Some(2_002),
            recorded_at_epoch_ms: Some(2_002),
            valid_from_epoch_ms: Some(2_002),
            valid_to_epoch_ms: None,
            last_verified_at_epoch_ms: Some(2_002),
            object_version: None,
            utility_score: Some(0.7),
            freshness_score: Some(0.8),
            retention_class: Some("standard"),
            ttl_epoch_ms: None,
            decay_policy: None,
            consolidation_status: None,
            imported_from: None,
            schema_version: Some("memory-envelope-v1"),
            superseded_by_memory_item_id: None,
            metadata: &json!({}),
            causation_id: None,
            correlation_id: None,
        },
    )
    .await
    .expect("target right item");
    let error = create_memory_conflict(
        &client,
        &target_project_code,
        "default",
        &MemoryConflictInsert {
            left_memory_item_id: Some(left.memory_item_id),
            right_memory_item_id: Some(right.memory_item_id),
            conflict_kind: "truth",
            conflict_state: Some("open"),
            severity: Some("high"),
            summary: "scope mismatch",
            evidence: &json!({"proof":"stage2"}),
            source_kind: Some("runtime_cli"),
            source_event_ids: Some(&json!([format!("event:memory-conflict-mismatch:{suffix}")])),
            artifact_refs: Some(&json!([format!(
                "artifact://proof/memory-conflict-mismatch/{suffix}"
            )])),
            message_refs: Some(&json!([format!(
                "message:memory-conflict-mismatch:{suffix}"
            )])),
            evidence_span: Some(&json!({"kind":"memory_conflict"})),
            derivation_kind: Some("extract"),
            schema_version: Some("memory-conflict-envelope-v1"),
            resolution: Some(&json!({})),
            detected_at_epoch_ms: Some(2_100),
            resolved_at_epoch_ms: None,
        },
    )
    .await
    .expect_err("left scope mismatch rejected");
    assert!(
        error
            .to_string()
            .contains("memory conflict left memory item scope does not match")
    );
}

#[tokio::test]
async fn create_memory_conflict_verification_conflict_check_detects_poisoned_evidence_span() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = trimmed.split_once('=') {
                unsafe {
                    std::env::set_var(key.trim(), value.trim_matches('\"'));
                }
            }
        }
    }

    let cfg = AppConfig::from_env().expect("config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let (_workspace_code, _source_project_code, target_project_code, _transfer_policy_code) =
        create_stage2_import_shared_context(&client, suffix).await;
    let left = create_memory_item(
        &client,
        &target_project_code,
        "default",
        &MemoryItemInsert {
            source_project_code: None,
            import_packet_id: None,
            item_kind: "fact",
            owner_agent_code: Some("stage2-proof"),
            identity_key: Some(&format!("memory-conflict-poison-left-{suffix}")),
            title: "left",
            summary: Some("left"),
            body: None,
            sensitivity_class: Some("internal"),
            truth_state: Some("current"),
            trust_state: Some("verified"),
            verification_state: Some("verified"),
            lifecycle_state: Some("hot"),
            source_event_ids: &[format!("event:memory-conflict-poison-left:{suffix}")],
            artifact_refs: &[format!(
                "artifact://proof/memory-conflict-poison-left/{suffix}"
            )],
            message_refs: &[format!("message:memory-conflict-poison-left:{suffix}")],
            evidence_span: &json!({"kind":"memory_conflict"}),
            derivation_kind: Some("extract"),
            observed_at_epoch_ms: Some(2_001),
            recorded_at_epoch_ms: Some(2_001),
            valid_from_epoch_ms: Some(2_001),
            valid_to_epoch_ms: None,
            last_verified_at_epoch_ms: Some(2_001),
            object_version: None,
            utility_score: Some(0.7),
            freshness_score: Some(0.8),
            retention_class: Some("standard"),
            ttl_epoch_ms: None,
            decay_policy: None,
            consolidation_status: None,
            imported_from: None,
            schema_version: Some("memory-envelope-v1"),
            superseded_by_memory_item_id: None,
            metadata: &json!({}),
            causation_id: None,
            correlation_id: None,
        },
    )
    .await
    .expect("left item");
    let right = create_memory_item(
        &client,
        &target_project_code,
        "default",
        &MemoryItemInsert {
            source_project_code: None,
            import_packet_id: None,
            item_kind: "fact",
            owner_agent_code: Some("stage2-proof"),
            identity_key: Some(&format!("memory-conflict-poison-right-{suffix}")),
            title: "right",
            summary: Some("right"),
            body: None,
            sensitivity_class: Some("internal"),
            truth_state: Some("current"),
            trust_state: Some("verified"),
            verification_state: Some("verified"),
            lifecycle_state: Some("hot"),
            source_event_ids: &[format!("event:memory-conflict-poison-right:{suffix}")],
            artifact_refs: &[format!(
                "artifact://proof/memory-conflict-poison-right/{suffix}"
            )],
            message_refs: &[format!("message:memory-conflict-poison-right:{suffix}")],
            evidence_span: &json!({"kind":"memory_conflict"}),
            derivation_kind: Some("extract"),
            observed_at_epoch_ms: Some(2_002),
            recorded_at_epoch_ms: Some(2_002),
            valid_from_epoch_ms: Some(2_002),
            valid_to_epoch_ms: None,
            last_verified_at_epoch_ms: Some(2_002),
            object_version: None,
            utility_score: Some(0.7),
            freshness_score: Some(0.8),
            retention_class: Some("standard"),
            ttl_epoch_ms: None,
            decay_policy: None,
            consolidation_status: None,
            imported_from: None,
            schema_version: Some("memory-envelope-v1"),
            superseded_by_memory_item_id: None,
            metadata: &json!({}),
            causation_id: None,
            correlation_id: None,
        },
    )
    .await
    .expect("right item");
    let error = create_memory_conflict(
        &client,
        &target_project_code,
        "default",
        &MemoryConflictInsert {
            left_memory_item_id: Some(left.memory_item_id),
            right_memory_item_id: Some(right.memory_item_id),
            conflict_kind: "truth",
            conflict_state: Some("open"),
            severity: Some("high"),
            summary: "poisoned",
            evidence: &json!({"proof":"stage2"}),
            source_kind: Some("runtime_cli"),
            source_event_ids: Some(&json!([format!("event:memory-conflict-poison:{suffix}")])),
            artifact_refs: Some(&json!([format!(
                "artifact://proof/memory-conflict-poison/{suffix}"
            )])),
            message_refs: Some(&json!([format!("message:memory-conflict-poison:{suffix}")])),
            evidence_span: Some(&json!({"kind":"memory_conflict","poisoned":true})),
            derivation_kind: Some("extract"),
            schema_version: Some("memory-conflict-envelope-v1"),
            resolution: Some(&json!({})),
            detected_at_epoch_ms: Some(2_100),
            resolved_at_epoch_ms: None,
        },
    )
    .await
    .expect_err("poisoned memory conflict rejected");
    assert!(
        error
            .to_string()
            .contains("memory conflict evidence_span is flagged poisoned")
    );
}

#[tokio::test]
async fn ensure_policy_surfaces_materialize_policy_rules() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = trimmed.split_once('=') {
                unsafe {
                    std::env::set_var(key.trim(), value.trim_matches('\"'));
                }
            }
        }
    }

    let cfg = AppConfig::from_env().expect("config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let workspace_code = format!("policy_ws_{suffix}");
    let project_code = format!("policy_proj_{suffix}");
    let repo_root = format!("/tmp/{project_code}");
    std::fs::create_dir_all(&repo_root).expect("repo root");

    ensure_workspace(&client, &workspace_code, "Policy Workspace", "active")
        .await
        .expect("workspace");
    upsert_project(
        &client,
        &project_code,
        "Policy Project",
        &repo_root,
        Some("main"),
        &workspace_code,
        "project_shared",
        "local_strict",
    )
    .await
    .expect("project");

    let transfer_policy = ensure_transfer_policy(
        &client,
        &workspace_code,
        &format!("policy_transfer_{suffix}"),
        "Policy transfer",
        "borrowed_unverified",
        true,
        true,
        true,
        false,
    )
    .await
    .expect("transfer policy");
    let access_policy = ensure_access_policy(
        &client,
        &workspace_code,
        None,
        None,
        Some(&project_code),
        &format!("policy_access_{suffix}"),
        "Policy access",
        "fact",
        "project_shared",
        250,
        true,
        true,
        true,
        true,
        true,
        true,
        true,
        false,
        true,
        true,
        false,
        None,
        "active",
    )
    .await
    .expect("access policy");

    let transfer_row = client
            .query_one(
                r#"
                SELECT rule_kind, source_kind, source_event_ids, evidence_span, derivation_kind, rule_payload
                FROM ami.policy_rules
                WHERE workspace_id = (SELECT workspace_id FROM ami.workspaces WHERE code = $1)
                  AND rule_code = $2
                "#,
                &[
                    &workspace_code,
                    &format!("transfer_policy:{}", transfer_policy.code),
                ],
            )
            .await
            .expect("transfer policy rule");
    assert_eq!(transfer_row.get::<_, String>(0), "import");
    assert_eq!(
        transfer_row.get::<_, Option<String>>(1).as_deref(),
        Some("transfer_policy_runtime")
    );
    assert_eq!(
        transfer_row.get::<_, Value>(2),
        json!([format!("transfer_policy:{}", transfer_policy.code)])
    );
    assert_eq!(
        transfer_row.get::<_, Value>(3)["kind"],
        json!("transfer_policy")
    );
    assert_eq!(transfer_row.get::<_, String>(4), "operator_write");
    assert_eq!(
        transfer_row.get::<_, Value>(5)["policy_surface"],
        json!("transfer_policy")
    );

    let access_row = client
            .query_one(
                r#"
                SELECT rule_scope, rule_kind, precedence, source_kind, source_event_ids, evidence_span, derivation_kind, rule_payload
                FROM ami.policy_rules
                WHERE workspace_id = (SELECT workspace_id FROM ami.workspaces WHERE code = $1)
                  AND rule_code = $2
                "#,
                &[
                    &workspace_code,
                    &format!("access_policy:{}", access_policy.code),
                ],
            )
            .await
            .expect("access policy rule");
    assert_eq!(access_row.get::<_, String>(0), "project");
    assert_eq!(access_row.get::<_, String>(1), "scope_filter");
    assert_eq!(access_row.get::<_, i32>(2), 250);
    assert_eq!(
        access_row.get::<_, Option<String>>(3).as_deref(),
        Some("access_policy_runtime")
    );
    assert_eq!(
        access_row.get::<_, Value>(4),
        json!([format!("access_policy:{}", access_policy.code)])
    );
    assert_eq!(
        access_row.get::<_, Value>(5)["kind"],
        json!("access_policy")
    );
    assert_eq!(access_row.get::<_, String>(6), "operator_write");
    assert_eq!(
        access_row.get::<_, Value>(7)["policy_surface"],
        json!("access_policy")
    );
}

#[tokio::test]
async fn create_policy_rule_surfaces_stage2_runtime_fields() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = trimmed.split_once('=') {
                unsafe {
                    std::env::set_var(key.trim(), value.trim_matches('\"'));
                }
            }
        }
    }

    let cfg = AppConfig::from_env().expect("config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let workspace_code = format!("policy_rule_ws_{suffix}");
    let project_code = format!("policy_rule_proj_{suffix}");
    let repo_root = format!("/tmp/{project_code}");
    std::fs::create_dir_all(&repo_root).expect("repo root");

    ensure_workspace(&client, &workspace_code, "Policy Rule Workspace", "active")
        .await
        .expect("workspace");
    let project = upsert_project(
        &client,
        &project_code,
        "Policy Rule Project",
        &repo_root,
        Some("main"),
        &workspace_code,
        "project_shared",
        "local_strict",
    )
    .await
    .expect("project");
    let namespace = ensure_namespace(
        &client,
        project.project_id,
        "review",
        Some("Review"),
        "local_strict",
    )
    .await
    .expect("namespace");

    let source_event_ids = json!([format!("event:policy-rule:{suffix}")]);
    let artifact_refs = json!([format!("artifact://proof/policy-rule/{suffix}")]);
    let message_refs = json!([format!("message:policy-rule:{suffix}")]);
    let evidence_span = json!({"kind":"policy_rule","surface":"runtime-test"});
    let rule = create_policy_rule(
        &client,
        &workspace_code,
        &PolicyRuleInsert {
            project_code: Some(&project_code),
            namespace_code: Some(&namespace.code),
            rule_code: &format!("policy-rule-{suffix}"),
            rule_scope: "project",
            rule_kind: "scope_filter",
            rule_status: Some("active"),
            precedence: Some(42),
            source_kind: Some("operator_panel"),
            source_event_ids: Some(&source_event_ids),
            artifact_refs: Some(&artifact_refs),
            message_refs: Some(&message_refs),
            evidence_span: Some(&evidence_span),
            derivation_kind: Some("operator_write"),
            schema_version: Some("policy-rule-envelope-v1"),
            rule_payload: &json!({"allow":["project_shared"],"deny":[]}),
        },
    )
    .await
    .expect("policy rule");

    assert_eq!(rule.workspace_code, workspace_code);
    assert_eq!(rule.project_code.as_deref(), Some(project_code.as_str()));
    assert_eq!(
        rule.namespace_code.as_deref(),
        Some(namespace.code.as_str())
    );
    assert_eq!(rule.rule_scope, "project");
    assert_eq!(rule.rule_kind, "scope_filter");
    assert_eq!(rule.source_kind.as_deref(), Some("operator_panel"));
    assert_eq!(rule.source_event_ids, source_event_ids);
    assert_eq!(rule.artifact_refs, artifact_refs);
    assert_eq!(rule.message_refs, message_refs);
    assert_eq!(rule.evidence_span["kind"], evidence_span["kind"]);
    assert_eq!(
        rule.evidence_span["stage2_runtime"]["policy_and_scope_filter"]["scope_binding_valid"],
        json!(true)
    );
    assert_eq!(
        rule.evidence_span["stage2_runtime"]["verification_conflict_check"]["write_allowed"],
        json!(true)
    );
}

#[tokio::test]
async fn create_policy_rule_policy_scope_filter_rejects_workspace_scope_with_project_binding() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = trimmed.split_once('=') {
                unsafe {
                    std::env::set_var(key.trim(), value.trim_matches('\"'));
                }
            }
        }
    }

    let cfg = AppConfig::from_env().expect("config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let workspace_code = format!("policy_rule_scope_ws_{suffix}");
    let project_code = format!("policy_rule_scope_proj_{suffix}");
    let repo_root = format!("/tmp/{project_code}");
    std::fs::create_dir_all(&repo_root).expect("repo root");

    ensure_workspace(
        &client,
        &workspace_code,
        "Policy Rule Scope Workspace",
        "active",
    )
    .await
    .expect("workspace");
    upsert_project(
        &client,
        &project_code,
        "Policy Rule Scope Project",
        &repo_root,
        Some("main"),
        &workspace_code,
        "project_shared",
        "local_strict",
    )
    .await
    .expect("project");

    let error = create_policy_rule(
        &client,
        &workspace_code,
        &PolicyRuleInsert {
            project_code: Some(&project_code),
            namespace_code: None,
            rule_code: &format!("policy-rule-invalid-scope-{suffix}"),
            rule_scope: "workspace",
            rule_kind: "scope_filter",
            rule_status: Some("active"),
            precedence: Some(10),
            source_kind: Some("operator_panel"),
            source_event_ids: Some(&json!([format!(
                "event:policy-rule-invalid-scope:{suffix}"
            )])),
            artifact_refs: Some(&json!([format!(
                "artifact://proof/policy-rule-invalid-scope/{suffix}"
            )])),
            message_refs: Some(&json!([format!(
                "message:policy-rule-invalid-scope:{suffix}"
            )])),
            evidence_span: Some(&json!({"kind":"policy_rule","case":"invalid-scope"})),
            derivation_kind: Some("operator_write"),
            schema_version: Some("policy-rule-envelope-v1"),
            rule_payload: &json!({"allow":["project_shared"],"deny":[]}),
        },
    )
    .await
    .expect_err("workspace scoped rule with project binding rejected");
    assert!(error.to_string().contains("invalid scope binding"));
}

#[tokio::test]
async fn create_policy_rule_verification_conflict_check_detects_poisoned_evidence_span() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = trimmed.split_once('=') {
                unsafe {
                    std::env::set_var(key.trim(), value.trim_matches('\"'));
                }
            }
        }
    }

    let cfg = AppConfig::from_env().expect("config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let workspace_code = format!("policy_rule_poison_ws_{suffix}");
    let project_code = format!("policy_rule_poison_proj_{suffix}");
    let repo_root = format!("/tmp/{project_code}");
    std::fs::create_dir_all(&repo_root).expect("repo root");

    ensure_workspace(
        &client,
        &workspace_code,
        "Policy Rule Poison Workspace",
        "active",
    )
    .await
    .expect("workspace");
    upsert_project(
        &client,
        &project_code,
        "Policy Rule Poison Project",
        &repo_root,
        Some("main"),
        &workspace_code,
        "project_shared",
        "local_strict",
    )
    .await
    .expect("project");

    let error = create_policy_rule(
        &client,
        &workspace_code,
        &PolicyRuleInsert {
            project_code: Some(&project_code),
            namespace_code: None,
            rule_code: &format!("policy-rule-poisoned-{suffix}"),
            rule_scope: "project",
            rule_kind: "scope_filter",
            rule_status: Some("active"),
            precedence: Some(20),
            source_kind: Some("operator_panel"),
            source_event_ids: Some(&json!([format!("event:policy-rule-poisoned:{suffix}")])),
            artifact_refs: Some(&json!([format!(
                "artifact://proof/policy-rule-poisoned/{suffix}"
            )])),
            message_refs: Some(&json!([format!("message:policy-rule-poisoned:{suffix}")])),
            evidence_span: Some(&json!({"kind":"policy_rule","poisoned":true})),
            derivation_kind: Some("operator_write"),
            schema_version: Some("policy-rule-envelope-v1"),
            rule_payload: &json!({"allow":["project_shared"],"deny":[]}),
        },
    )
    .await
    .expect_err("poisoned policy rule rejected");
    assert!(error.to_string().contains("flagged poisoned"));
}

#[tokio::test]
async fn create_quarantine_item_surfaces_stage2_runtime_fields() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = trimmed.split_once('=') {
                unsafe {
                    std::env::set_var(key.trim(), value.trim_matches('\"'));
                }
            }
        }
    }

    let cfg = AppConfig::from_env().expect("config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let workspace_code = format!("quarantine_rule_ws_{suffix}");
    let project_code = format!("quarantine_rule_proj_{suffix}");
    let repo_root = format!("/tmp/{project_code}");
    std::fs::create_dir_all(&repo_root).expect("repo root");

    ensure_workspace(
        &client,
        &workspace_code,
        "Quarantine Rule Workspace",
        "active",
    )
    .await
    .expect("workspace");
    let project = upsert_project(
        &client,
        &project_code,
        "Quarantine Rule Project",
        &repo_root,
        Some("main"),
        &workspace_code,
        "project_shared",
        "local_strict",
    )
    .await
    .expect("project");
    let namespace = ensure_namespace(
        &client,
        project.project_id,
        "review",
        Some("Review"),
        "local_strict",
    )
    .await
    .expect("namespace");

    let policy_rule = create_policy_rule(
        &client,
        &workspace_code,
        &PolicyRuleInsert {
            project_code: Some(&project_code),
            namespace_code: Some(&namespace.code),
            rule_code: &format!("quarantine-policy-rule-{suffix}"),
            rule_scope: "project",
            rule_kind: "scope_filter",
            rule_status: Some("active"),
            precedence: Some(100),
            source_kind: Some("operator_panel"),
            source_event_ids: Some(&json!([format!("event:policy-rule:{suffix}")])),
            artifact_refs: Some(&json!([format!("artifact://proof/policy-rule/{suffix}")])),
            message_refs: Some(&json!([format!("message:policy-rule:{suffix}")])),
            evidence_span: Some(&json!({"kind":"policy_rule","surface":"quarantine-test"})),
            derivation_kind: Some("operator_write"),
            schema_version: Some("policy-rule-envelope-v1"),
            rule_payload: &json!({"allow":["project_shared"],"deny":[]}),
        },
    )
    .await
    .expect("policy rule");

    let source_event_ids = json!([format!("event:quarantine-item:{suffix}")]);
    let artifact_refs = json!([format!("artifact://proof/quarantine-item/{suffix}")]);
    let message_refs = json!([format!("message:quarantine-item:{suffix}")]);
    let evidence_span = json!({"kind":"quarantine_item","surface":"runtime-test"});
    let item = create_quarantine_item(
        &client,
        &workspace_code,
        &QuarantineItemInsert {
            project_code: Some(&project_code),
            namespace_code: Some(&namespace.code),
            entity_kind: "policy_rule",
            entity_id: Some(policy_rule.policy_rule_id),
            quarantine_reason: "runtime quarantine",
            quarantine_state: Some("active"),
            evidence: &json!({"surface":"quarantine_item","proof":"stage2"}),
            source_kind: Some("operator_panel"),
            source_event_ids: Some(&source_event_ids),
            artifact_refs: Some(&artifact_refs),
            message_refs: Some(&message_refs),
            evidence_span: Some(&evidence_span),
            derivation_kind: Some("operator_write"),
            schema_version: Some("quarantine-item-envelope-v1"),
            quarantined_at_epoch_ms: Some(7_100),
            released_at_epoch_ms: None,
        },
    )
    .await
    .expect("quarantine item");

    assert_eq!(item.workspace_code, workspace_code);
    assert_eq!(item.project_code.as_deref(), Some(project_code.as_str()));
    assert_eq!(
        item.namespace_code.as_deref(),
        Some(namespace.code.as_str())
    );
    assert_eq!(item.entity_kind, "policy_rule");
    assert_eq!(item.entity_id, Some(policy_rule.policy_rule_id));
    assert_eq!(item.source_event_ids, source_event_ids);
    assert_eq!(item.artifact_refs, artifact_refs);
    assert_eq!(item.message_refs, message_refs);
    assert_eq!(item.evidence_span["kind"], evidence_span["kind"]);
    assert_eq!(
        item.evidence_span["stage2_runtime"]["policy_and_scope_filter"]["scope_binding_valid"],
        json!(true)
    );
    assert_eq!(
        item.evidence_span["stage2_runtime"]["verification_conflict_check"]["write_allowed"],
        json!(true)
    );
}

#[tokio::test]
async fn create_quarantine_item_policy_scope_filter_rejects_missing_entity_id_for_policy_rule() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = trimmed.split_once('=') {
                unsafe {
                    std::env::set_var(key.trim(), value.trim_matches('\"'));
                }
            }
        }
    }

    let cfg = AppConfig::from_env().expect("config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let workspace_code = format!("quarantine_scope_ws_{suffix}");

    ensure_workspace(
        &client,
        &workspace_code,
        "Quarantine Scope Workspace",
        "active",
    )
    .await
    .expect("workspace");

    let error = create_quarantine_item(
        &client,
        &workspace_code,
        &QuarantineItemInsert {
            project_code: None,
            namespace_code: None,
            entity_kind: "policy_rule",
            entity_id: None,
            quarantine_reason: "missing entity id",
            quarantine_state: Some("active"),
            evidence: &json!({"surface":"quarantine_item","proof":"stage2"}),
            source_kind: Some("operator_panel"),
            source_event_ids: Some(&json!([format!("event:quarantine-scope:{suffix}")])),
            artifact_refs: Some(&json!([format!(
                "artifact://proof/quarantine-scope/{suffix}"
            )])),
            message_refs: Some(&json!([format!("message:quarantine-scope:{suffix}")])),
            evidence_span: Some(&json!({"kind":"quarantine_item","case":"missing-entity-id"})),
            derivation_kind: Some("operator_write"),
            schema_version: Some("quarantine-item-envelope-v1"),
            quarantined_at_epoch_ms: Some(7_100),
            released_at_epoch_ms: None,
        },
    )
    .await
    .expect_err("missing entity_id rejected");
    assert!(error.to_string().contains("requires entity_id"));
}

#[tokio::test]
async fn create_quarantine_item_verification_conflict_check_detects_poisoned_evidence_span() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = trimmed.split_once('=') {
                unsafe {
                    std::env::set_var(key.trim(), value.trim_matches('\"'));
                }
            }
        }
    }

    let cfg = AppConfig::from_env().expect("config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let workspace_code = format!("quarantine_poison_ws_{suffix}");

    ensure_workspace(
        &client,
        &workspace_code,
        "Quarantine Poison Workspace",
        "active",
    )
    .await
    .expect("workspace");

    let error = create_quarantine_item(
        &client,
        &workspace_code,
        &QuarantineItemInsert {
            project_code: None,
            namespace_code: None,
            entity_kind: "other",
            entity_id: None,
            quarantine_reason: "poisoned evidence",
            quarantine_state: Some("active"),
            evidence: &json!({"surface":"quarantine_item","proof":"stage2"}),
            source_kind: Some("operator_panel"),
            source_event_ids: Some(&json!([format!("event:quarantine-poison:{suffix}")])),
            artifact_refs: Some(&json!([format!(
                "artifact://proof/quarantine-poison/{suffix}"
            )])),
            message_refs: Some(&json!([format!("message:quarantine-poison:{suffix}")])),
            evidence_span: Some(&json!({"kind":"quarantine_item","poisoned":true})),
            derivation_kind: Some("operator_write"),
            schema_version: Some("quarantine-item-envelope-v1"),
            quarantined_at_epoch_ms: Some(7_100),
            released_at_epoch_ms: None,
        },
    )
    .await
    .expect_err("poisoned quarantine item rejected");
    assert!(error.to_string().contains("flagged poisoned"));
}

#[tokio::test]
async fn update_import_packet_quarantine_materializes_and_releases_quarantine_item() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = trimmed.split_once('=') {
                unsafe {
                    std::env::set_var(key.trim(), value.trim_matches('\"'));
                }
            }
        }
    }

    let cfg = AppConfig::from_env().expect("config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let workspace_code = format!("quarantine_ws_{suffix}");
    let source_code = format!("quarantine_source_{suffix}");
    let target_code = format!("quarantine_target_{suffix}");
    let source_root = format!("/tmp/{source_code}");
    let target_root = format!("/tmp/{target_code}");
    std::fs::create_dir_all(&source_root).expect("source root");
    std::fs::create_dir_all(&target_root).expect("target root");

    ensure_workspace(&client, &workspace_code, "Quarantine Workspace", "active")
        .await
        .expect("workspace");
    upsert_project(
        &client,
        &source_code,
        "Quarantine Source",
        &source_root,
        Some("main"),
        &workspace_code,
        "project_shared",
        "local_strict",
    )
    .await
    .expect("source project");
    upsert_project(
        &client,
        &target_code,
        "Quarantine Target",
        &target_root,
        Some("main"),
        &workspace_code,
        "project_shared",
        "local_strict",
    )
    .await
    .expect("target project");
    let transfer_policy = ensure_transfer_policy(
        &client,
        &workspace_code,
        &format!("quarantine_transfer_{suffix}"),
        "Quarantine transfer",
        "borrowed_unverified",
        true,
        true,
        true,
        false,
    )
    .await
    .expect("transfer policy");
    ensure_access_policy(
        &client,
        &workspace_code,
        None,
        None,
        Some(&source_code),
        &format!("quarantine_access_{suffix}"),
        "Quarantine access",
        "fact",
        "cross_project_linked",
        250,
        true,
        true,
        true,
        true,
        true,
        true,
        true,
        false,
        true,
        true,
        false,
        None,
        "active",
    )
    .await
    .expect("access policy");
    add_relation(
        &client,
        &source_code,
        &target_code,
        "knowledge_may_transfer",
        Some("knowledge_may_transfer"),
        "memory_transfer",
        "cross_project_linked",
        "active",
        false,
        Some(transfer_policy.code.as_str()),
        "local_plus_related",
    )
    .await
    .expect("relation");
    let packet = create_import_packet(
        &client,
        &source_code,
        &target_code,
        Some(transfer_policy.code.as_str()),
        None,
        "borrowed_unverified",
        Some("quarantine packet"),
        Some("quarantine runtime proof"),
        "cross_project_linked",
        "proposed",
        "unverified",
        "borrowed",
        true,
        &[format!("memory_item:{suffix}")],
        &[format!("file:///tmp/quarantine_artifact_{suffix}.md")],
        Some("import_runtime"),
        Some(&json!([format!("import_event:{suffix}")])),
        Some(&json!([format!("thread:{suffix}")])),
        Some(&json!({"kind":"import_packet_runtime","suffix":suffix})),
        Some("import"),
        Some("import-packet-envelope-v1"),
    )
    .await
    .expect("import packet");

    update_import_packet(
        &client,
        ImportPacketUpdate {
            import_packet_id: packet.import_packet_id,
            status: Some("quarantined"),
            summary: Some("quarantine enforced"),
            reason: Some("manual quarantine"),
            imported_by_agent_scope: None,
            trust_state: Some("disputed"),
            verification_state: Some("rejected"),
            borrowed_status: Some("rejected"),
            can_promote_after_verification: Some(false),
            actor_agent_code: None,
        },
    )
    .await
    .expect("quarantine packet");

    let quarantine_row = client
            .query_one(
                r#"
                SELECT entity_kind, quarantine_state, source_kind, source_event_ids, evidence_span, derivation_kind
                FROM ami.quarantine_items
                WHERE entity_kind = 'import_packet'
                  AND entity_id = $1
                ORDER BY created_at DESC
                LIMIT 1
                "#,
                &[&packet.import_packet_id],
            )
            .await
            .expect("quarantine row");
    assert_eq!(quarantine_row.get::<_, String>(0), "import_packet");
    assert_eq!(quarantine_row.get::<_, String>(1), "active");
    assert_eq!(
        quarantine_row.get::<_, Option<String>>(2).as_deref(),
        Some("import_packet_override")
    );
    assert_eq!(
        quarantine_row.get::<_, Value>(3),
        json!([format!("import_packet:{}", packet.import_packet_id)])
    );
    assert_eq!(
        quarantine_row.get::<_, Value>(4)["kind"],
        json!("import_packet_quarantine")
    );
    assert_eq!(quarantine_row.get::<_, String>(5), "operator_write");

    update_import_packet(
        &client,
        ImportPacketUpdate {
            import_packet_id: packet.import_packet_id,
            status: Some("verified"),
            summary: Some("quarantine released"),
            reason: Some("verification complete"),
            imported_by_agent_scope: Some("cross_project_linked"),
            trust_state: Some("verified"),
            verification_state: Some("verified"),
            borrowed_status: Some("verified_local_copy"),
            can_promote_after_verification: Some(true),
            actor_agent_code: None,
        },
    )
    .await
    .expect("release packet quarantine");

    let released_state: String = client
        .query_one(
            r#"
                SELECT quarantine_state
                FROM ami.quarantine_items
                WHERE entity_kind = 'import_packet'
                  AND entity_id = $1
                ORDER BY created_at DESC
                LIMIT 1
                "#,
            &[&packet.import_packet_id],
        )
        .await
        .expect("released quarantine state")
        .get(0);
    assert_eq!(released_state, "released");
}

#[tokio::test]
async fn reconcile_import_packet_quarantines_autonomously_releases_clean_packet() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = trimmed.split_once('=') {
                unsafe {
                    std::env::set_var(key.trim(), value.trim_matches('\"'));
                }
            }
        }
    }

    let cfg = AppConfig::from_env().expect("config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let workspace_code = format!("auto_release_ws_{suffix}");
    let source_code = format!("auto_release_source_{suffix}");
    let target_code = format!("auto_release_target_{suffix}");
    let source_root = format!("/tmp/{source_code}");
    let target_root = format!("/tmp/{target_code}");
    std::fs::create_dir_all(&source_root).expect("source root");
    std::fs::create_dir_all(&target_root).expect("target root");

    ensure_workspace(&client, &workspace_code, "Auto Release Workspace", "active")
        .await
        .expect("workspace");
    upsert_project(
        &client,
        &source_code,
        "Auto Release Source",
        &source_root,
        Some("main"),
        &workspace_code,
        "project_shared",
        "local_strict",
    )
    .await
    .expect("source project");
    upsert_project(
        &client,
        &target_code,
        "Auto Release Target",
        &target_root,
        Some("main"),
        &workspace_code,
        "project_shared",
        "local_strict",
    )
    .await
    .expect("target project");
    let transfer_policy = ensure_transfer_policy(
        &client,
        &workspace_code,
        &format!("auto_release_transfer_{suffix}"),
        "Auto release transfer",
        "verified_writeback",
        true,
        true,
        true,
        false,
    )
    .await
    .expect("transfer policy");
    ensure_access_policy(
        &client,
        &workspace_code,
        None,
        None,
        Some(&source_code),
        &format!("auto_release_access_{suffix}"),
        "Auto release access",
        "fact",
        "cross_project_linked",
        250,
        true,
        true,
        true,
        true,
        true,
        true,
        true,
        false,
        true,
        true,
        false,
        None,
        "active",
    )
    .await
    .expect("access policy");
    add_relation(
        &client,
        &source_code,
        &target_code,
        "knowledge_may_transfer",
        Some("knowledge_may_transfer"),
        "memory_transfer",
        "cross_project_linked",
        "active",
        false,
        Some(transfer_policy.code.as_str()),
        "local_plus_related",
    )
    .await
    .expect("relation");
    let packet = create_import_packet(
        &client,
        &source_code,
        &target_code,
        Some(transfer_policy.code.as_str()),
        None,
        "borrowed_unverified",
        Some("auto release packet"),
        Some("initial import"),
        "cross_project_linked",
        "proposed",
        "unverified",
        "borrowed",
        false,
        &[format!("memory_item:{suffix}")],
        &[format!("file:///tmp/auto_release_artifact_{suffix}.md")],
        Some("import_runtime"),
        Some(&json!([format!("import_event:{suffix}")])),
        Some(&json!([format!("thread:{suffix}")])),
        Some(&json!({"kind":"import_packet_runtime","suffix":suffix})),
        Some("import"),
        Some("import-packet-envelope-v1"),
    )
    .await
    .expect("packet");

    update_import_packet(
        &client,
        ImportPacketUpdate {
            import_packet_id: packet.import_packet_id,
            status: Some("quarantined"),
            summary: Some("manual quarantine requested"),
            reason: Some("manual quarantine"),
            imported_by_agent_scope: None,
            trust_state: Some("disputed"),
            verification_state: Some("unverified"),
            borrowed_status: Some("borrowed"),
            can_promote_after_verification: Some(false),
            actor_agent_code: None,
        },
    )
    .await
    .expect("quarantine packet");

    let summary = reconcile_import_packet_quarantines(&client, true, Some(8))
        .await
        .expect("reconcile");
    assert!(summary.released >= 1);
    let decision = summary
        .decisions
        .iter()
        .find(|decision| decision.import_packet_id == packet.import_packet_id)
        .expect("decision for released packet");
    assert_eq!(decision.decision, "release");

    let packet = get_import_packet(&client, packet.import_packet_id)
        .await
        .expect("packet after release");
    assert_eq!(packet.status, "verified");
    assert_eq!(packet.verification_state, "verified");
    assert_eq!(packet.borrowed_status, "verified_local_copy");
    assert!(packet.can_promote_after_verification);

    let quarantine_state: String = client
        .query_one(
            r#"
                SELECT quarantine_state
                FROM ami.quarantine_items
                WHERE entity_kind = 'import_packet'
                  AND entity_id = $1
                ORDER BY created_at DESC
                LIMIT 1
                "#,
            &[&packet.import_packet_id],
        )
        .await
        .expect("quarantine state")
        .get(0);
    assert_eq!(quarantine_state, "released");
}

#[tokio::test]
async fn reconcile_import_packet_quarantines_releases_stale_verified_quarantine_item() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = trimmed.split_once('=') {
                unsafe {
                    std::env::set_var(key.trim(), value.trim_matches('\"'));
                }
            }
        }
    }

    let cfg = AppConfig::from_env().expect("config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let workspace_code = format!("stale_quarantine_ws_{suffix}");
    let source_code = format!("stale_quarantine_source_{suffix}");
    let target_code = format!("stale_quarantine_target_{suffix}");
    let source_root = format!("/tmp/{source_code}");
    let target_root = format!("/tmp/{target_code}");
    std::fs::create_dir_all(&source_root).expect("source root");
    std::fs::create_dir_all(&target_root).expect("target root");

    ensure_workspace(
        &client,
        &workspace_code,
        "Stale Quarantine Workspace",
        "active",
    )
    .await
    .expect("workspace");
    upsert_project(
        &client,
        &source_code,
        "Stale Quarantine Source",
        &source_root,
        Some("main"),
        &workspace_code,
        "project_shared",
        "local_strict",
    )
    .await
    .expect("source project");
    upsert_project(
        &client,
        &target_code,
        "Stale Quarantine Target",
        &target_root,
        Some("main"),
        &workspace_code,
        "project_shared",
        "local_strict",
    )
    .await
    .expect("target project");
    let transfer_policy = ensure_transfer_policy(
        &client,
        &workspace_code,
        &format!("stale_quarantine_transfer_{suffix}"),
        "Stale quarantine transfer",
        "verified_writeback",
        true,
        true,
        true,
        false,
    )
    .await
    .expect("transfer policy");
    ensure_access_policy(
        &client,
        &workspace_code,
        None,
        None,
        Some(&source_code),
        &format!("stale_quarantine_access_{suffix}"),
        "Stale quarantine access",
        "fact",
        "cross_project_linked",
        250,
        true,
        true,
        true,
        true,
        true,
        true,
        true,
        false,
        true,
        false,
        false,
        None,
        "active",
    )
    .await
    .expect("access policy");
    add_relation(
        &client,
        &source_code,
        &target_code,
        "knowledge_may_transfer",
        Some("knowledge_may_transfer"),
        "memory_transfer",
        "cross_project_linked",
        "active",
        false,
        Some(transfer_policy.code.as_str()),
        "local_plus_related",
    )
    .await
    .expect("relation");
    let packet = create_import_packet(
        &client,
        &source_code,
        &target_code,
        Some(transfer_policy.code.as_str()),
        None,
        "verified",
        Some("stale quarantine packet"),
        Some("verified packet with stale quarantine"),
        "cross_project_linked",
        "verified",
        "verified",
        "verified_local_copy",
        true,
        &[format!("memory_item:{suffix}")],
        &[format!("file:///tmp/stale_quarantine_artifact_{suffix}.md")],
        Some("import_runtime"),
        Some(&json!([format!("import_event:{suffix}")])),
        Some(&json!([format!("thread:{suffix}")])),
        Some(&json!({"kind":"import_packet_runtime","suffix":suffix})),
        Some("import"),
        Some("import-packet-envelope-v1"),
    )
    .await
    .expect("packet");
    let _ = create_quarantine_item(
        &client,
        &workspace_code,
        &QuarantineItemInsert {
            project_code: Some(&target_code),
            namespace_code: None,
            entity_kind: "import_packet",
            entity_id: Some(packet.import_packet_id),
            quarantine_reason: "manual quarantine",
            quarantine_state: Some("active"),
            evidence: &json!({"kind":"stale_verified_quarantine"}),
            source_kind: Some("import"),
            source_event_ids: Some(&json!([format!("import_event:{suffix}")])),
            artifact_refs: Some(&json!([format!(
                "file:///tmp/stale_quarantine_artifact_{suffix}.md"
            )])),
            message_refs: Some(&json!([format!("thread:{suffix}")])),
            evidence_span: Some(&json!({
                "kind":"import_packet_runtime",
                "suffix": suffix,
                "source_event_ids":[format!("import_event:{suffix}")],
                "artifact_refs":[format!("file:///tmp/stale_quarantine_artifact_{suffix}.md")]
            })),
            derivation_kind: Some("import"),
            schema_version: Some("quarantine-item-envelope-v1"),
            quarantined_at_epoch_ms: None,
            released_at_epoch_ms: None,
        },
    )
    .await
    .expect("stale quarantine");

    let summary = reconcile_import_packet_quarantines(&client, true, Some(8))
        .await
        .expect("reconcile");
    assert!(summary.released >= 1);
    let decision = summary
        .decisions
        .iter()
        .find(|decision| decision.import_packet_id == packet.import_packet_id)
        .expect("decision for stale verified packet");
    assert_eq!(decision.decision, "release");
    assert!(decision.reason.contains("packet already verified"));

    let quarantine_state: String = client
        .query_one(
            r#"
                SELECT quarantine_state
                FROM ami.quarantine_items
                WHERE entity_kind = 'import_packet'
                  AND entity_id = $1
                ORDER BY created_at DESC
                LIMIT 1
                "#,
            &[&packet.import_packet_id],
        )
        .await
        .expect("quarantine state")
        .get(0);
    assert_eq!(quarantine_state, "released");
}

#[tokio::test]
async fn reconcile_import_packet_quarantines_rejects_approval_gated_packet() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = trimmed.split_once('=') {
                unsafe {
                    std::env::set_var(key.trim(), value.trim_matches('\"'));
                }
            }
        }
    }

    let cfg = AppConfig::from_env().expect("config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let workspace_code = format!("auto_reject_ws_{suffix}");
    let source_code = format!("auto_reject_source_{suffix}");
    let target_code = format!("auto_reject_target_{suffix}");
    let source_root = format!("/tmp/{source_code}");
    let target_root = format!("/tmp/{target_code}");
    std::fs::create_dir_all(&source_root).expect("source root");
    std::fs::create_dir_all(&target_root).expect("target root");

    ensure_workspace(&client, &workspace_code, "Auto Reject Workspace", "active")
        .await
        .expect("workspace");
    upsert_project(
        &client,
        &source_code,
        "Auto Reject Source",
        &source_root,
        Some("main"),
        &workspace_code,
        "project_shared",
        "local_strict",
    )
    .await
    .expect("source project");
    upsert_project(
        &client,
        &target_code,
        "Auto Reject Target",
        &target_root,
        Some("main"),
        &workspace_code,
        "project_shared",
        "local_strict",
    )
    .await
    .expect("target project");
    let transfer_policy = ensure_transfer_policy(
        &client,
        &workspace_code,
        &format!("auto_reject_transfer_{suffix}"),
        "Auto reject transfer",
        "manual_review",
        true,
        true,
        true,
        true,
    )
    .await
    .expect("transfer policy");
    ensure_access_policy(
        &client,
        &workspace_code,
        None,
        None,
        Some(&source_code),
        &format!("auto_reject_access_{suffix}"),
        "Auto reject access",
        "fact",
        "cross_project_linked",
        250,
        true,
        true,
        true,
        true,
        true,
        true,
        true,
        false,
        true,
        true,
        false,
        None,
        "active",
    )
    .await
    .expect("access policy");
    add_relation(
        &client,
        &source_code,
        &target_code,
        "knowledge_may_transfer",
        Some("knowledge_may_transfer"),
        "memory_transfer",
        "cross_project_linked",
        "active",
        true,
        Some(transfer_policy.code.as_str()),
        "local_plus_related",
    )
    .await
    .expect("relation");
    let packet = create_import_packet(
        &client,
        &source_code,
        &target_code,
        Some(transfer_policy.code.as_str()),
        None,
        "borrowed_unverified",
        Some("auto reject packet"),
        Some("initial import"),
        "cross_project_linked",
        "proposed",
        "unverified",
        "borrowed",
        false,
        &[format!("memory_item:{suffix}")],
        &[format!("file:///tmp/auto_reject_artifact_{suffix}.md")],
        Some("import_runtime"),
        Some(&json!([format!("import_event:{suffix}")])),
        Some(&json!([format!("thread:{suffix}")])),
        Some(&json!({"kind":"import_packet_runtime","suffix":suffix})),
        Some("import"),
        Some("import-packet-envelope-v1"),
    )
    .await
    .expect("packet");

    update_import_packet(
        &client,
        ImportPacketUpdate {
            import_packet_id: packet.import_packet_id,
            status: Some("quarantined"),
            summary: Some("manual quarantine requested"),
            reason: Some("manual quarantine"),
            imported_by_agent_scope: None,
            trust_state: Some("disputed"),
            verification_state: Some("unverified"),
            borrowed_status: Some("borrowed"),
            can_promote_after_verification: Some(false),
            actor_agent_code: None,
        },
    )
    .await
    .expect("quarantine packet");

    let summary = reconcile_import_packet_quarantines(&client, true, Some(8))
        .await
        .expect("reconcile");
    assert!(summary.rejected >= 1);
    let decision = summary
        .decisions
        .iter()
        .find(|decision| decision.import_packet_id == packet.import_packet_id)
        .expect("decision for rejected packet");
    assert_eq!(decision.decision, "reject");
    assert!(!decision.reason.trim().is_empty());

    let packet = get_import_packet(&client, packet.import_packet_id)
        .await
        .expect("packet after reject");
    assert_eq!(packet.status, "rejected");
    assert_eq!(packet.verification_state, "rejected");
    assert_eq!(packet.borrowed_status, "rejected");
    assert!(!packet.can_promote_after_verification);

    let quarantine_state: String = client
        .query_one(
            r#"
                SELECT quarantine_state
                FROM ami.quarantine_items
                WHERE entity_kind = 'import_packet'
                  AND entity_id = $1
                ORDER BY created_at DESC
                LIMIT 1
                "#,
            &[&packet.import_packet_id],
        )
        .await
        .expect("quarantine state")
        .get(0);
    assert_eq!(quarantine_state, "rejected");
}

#[tokio::test]
async fn update_relation_quarantine_materializes_and_resolves_quarantine_item() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = trimmed.split_once('=') {
                unsafe {
                    std::env::set_var(key.trim(), value.trim_matches('\"'));
                }
            }
        }
    }

    let cfg = AppConfig::from_env().expect("config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let workspace_code = format!("relation_quarantine_ws_{suffix}");
    let source_code = format!("relation_source_{suffix}");
    let target_code = format!("relation_target_{suffix}");
    let source_root = format!("/tmp/{source_code}");
    let target_root = format!("/tmp/{target_code}");
    std::fs::create_dir_all(&source_root).expect("source root");
    std::fs::create_dir_all(&target_root).expect("target root");

    ensure_workspace(
        &client,
        &workspace_code,
        "Relation Quarantine Workspace",
        "active",
    )
    .await
    .expect("workspace");
    upsert_project(
        &client,
        &source_code,
        "Relation Source",
        &source_root,
        Some("main"),
        &workspace_code,
        "project_shared",
        "local_strict",
    )
    .await
    .expect("source project");
    upsert_project(
        &client,
        &target_code,
        "Relation Target",
        &target_root,
        Some("main"),
        &workspace_code,
        "project_shared",
        "local_strict",
    )
    .await
    .expect("target project");
    add_relation(
        &client,
        &source_code,
        &target_code,
        "knowledge_may_transfer",
        Some("knowledge_may_transfer"),
        "memory_transfer",
        "cross_project_linked",
        "active",
        false,
        None,
        "local_plus_related",
    )
    .await
    .expect("relation");

    update_relation(
        &client,
        RelationUpdate {
            source_code: &source_code,
            target_code: &target_code,
            relation_type: "knowledge_may_transfer",
            shared_contour: "memory_transfer",
            project_link_type: Some("knowledge_may_transfer"),
            visibility_scope: Some("quarantine"),
            relation_status: Some("quarantined"),
            requires_approval: Some(true),
            transfer_policy_code: None,
            access_mode: Some("local_plus_related"),
            actor_agent_code: None,
            override_reason: Some("relation quarantine"),
        },
    )
    .await
    .expect("quarantine relation");

    let quarantine_row = client
            .query_one(
                r#"
                SELECT entity_kind, quarantine_state, source_kind, source_event_ids, evidence_span, derivation_kind
                FROM ami.quarantine_items
                WHERE entity_kind = 'project_relation'
                ORDER BY created_at DESC
                LIMIT 1
                "#,
                &[],
            )
            .await
            .expect("relation quarantine row");
    assert_eq!(quarantine_row.get::<_, String>(0), "project_relation");
    assert_eq!(quarantine_row.get::<_, String>(1), "active");
    assert_eq!(
        quarantine_row.get::<_, Option<String>>(2).as_deref(),
        Some("project_relation_override")
    );
    assert_eq!(
        quarantine_row.get::<_, Value>(3),
        json!([format!(
            "project_relation:{}:{}:{}:{}",
            source_code, target_code, "knowledge_may_transfer", "memory_transfer"
        )])
    );
    assert_eq!(
        quarantine_row.get::<_, Value>(4)["kind"],
        json!("project_relation_quarantine")
    );
    assert_eq!(quarantine_row.get::<_, String>(5), "operator_write");

    update_relation(
        &client,
        RelationUpdate {
            source_code: &source_code,
            target_code: &target_code,
            relation_type: "knowledge_may_transfer",
            shared_contour: "memory_transfer",
            project_link_type: Some("knowledge_may_transfer"),
            visibility_scope: Some("cross_project_linked"),
            relation_status: Some("active"),
            requires_approval: Some(false),
            transfer_policy_code: None,
            access_mode: Some("local_plus_related"),
            actor_agent_code: None,
            override_reason: Some("relation restored"),
        },
    )
    .await
    .expect("release relation quarantine");

    let released_state: String = client
        .query_one(
            r#"
                SELECT quarantine_state
                FROM ami.quarantine_items
                WHERE entity_kind = 'project_relation'
                ORDER BY created_at DESC
                LIMIT 1
                "#,
            &[],
        )
        .await
        .expect("relation quarantine state")
        .get(0);
    assert_eq!(released_state, "released");
}

#[tokio::test]
async fn create_memory_relation_edge_surfaces_stage2_provenance_fields() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            };
            let Some((key, value)) = trimmed.split_once('=') else {
                continue;
            };
            if std::env::var_os(key).is_none() {
                unsafe {
                    std::env::set_var(key, value);
                }
            }
        }
    }
    unsafe {
        std::env::set_var("AMI_STACK_NAME", "default");
    }
    let cfg = AppConfig::from_env().expect("env config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("unix epoch")
        .as_nanos();
    let provenance = json!({
        "source_event_ids": [format!("event:relation-card:{suffix}")],
        "artifact_refs": [format!("artifact://proof/relation-card/{suffix}")],
        "message_refs": [format!("thread:relation-card:{suffix}")],
        "evidence_span": {"kind":"memory_card","suffix":suffix},
        "source_kind": "relation_card_seed",
    });
    let source_card = create_memory_card(
        &client,
        "project_alpha",
        "review",
        &format!("relation source {suffix}"),
        "summary",
        "body",
        &[],
        &provenance,
        Some(&format!("subject-a:{suffix}")),
        Some(&format!("predicate:{suffix}")),
        Some(&format!("object-a:{suffix}")),
        Some("current"),
        Some("verified"),
        Some("active"),
        Some(1000),
        Some(1001),
        Some(1000),
        None,
        Some(1002),
    )
    .await
    .expect("source card");
    let target_card = create_memory_card(
        &client,
        "project_alpha",
        "review",
        &format!("relation target {suffix}"),
        "summary",
        "body",
        &[],
        &provenance,
        Some(&format!("subject-b:{suffix}")),
        Some(&format!("predicate:{suffix}")),
        Some(&format!("object-b:{suffix}")),
        Some("current"),
        Some("verified"),
        Some("active"),
        Some(1000),
        Some(1001),
        Some(1000),
        None,
        Some(1002),
    )
    .await
    .expect("target card");
    let source_event_ids = json!([format!("event:relation-edge:{suffix}")]);
    let artifact_refs = json!([format!("artifact://proof/relation-edge/{suffix}")]);
    let message_refs = json!([format!("thread:relation-edge:{suffix}")]);
    let evidence_span = json!({
        "kind": "memory_relation_edge",
        "source_card": source_card.memory_card_id,
        "target_card": target_card.memory_card_id,
    });
    let relation = create_memory_relation_edge(
        &client,
        "project_alpha",
        "review",
        source_card.memory_card_id,
        target_card.memory_card_id,
        "supports",
        Some("active"),
        &json!({"proof":"stage2","kind":"relation"}),
        Some("relation_graph_extract"),
        Some(&source_event_ids),
        Some(&artifact_refs),
        Some(&message_refs),
        Some(&evidence_span),
        Some("extract"),
        Some("memory-relation-edge-envelope-v1"),
        Some(1003),
        Some(1003),
        None,
    )
    .await
    .expect("memory relation edge");
    assert_eq!(
        relation.source_kind.as_deref(),
        Some("relation_graph_extract")
    );
    assert_eq!(relation.source_event_ids, source_event_ids);
    assert_eq!(relation.artifact_refs, artifact_refs);
    assert_eq!(relation.message_refs, message_refs);
    assert_eq!(relation.evidence_span["kind"], evidence_span["kind"]);
    assert_eq!(
        relation.evidence_span["stage2_runtime"]["policy_and_scope_filter"]["scope_binding_valid"],
        json!(true)
    );
    assert_eq!(
        relation.evidence_span["stage2_runtime"]["verification_conflict_check"]["write_allowed"],
        json!(true)
    );
    assert_eq!(relation.derivation_kind, "extract");
    assert_eq!(relation.schema_version, "memory-relation-edge-envelope-v1");
}

#[tokio::test]
async fn create_memory_relation_edge_policy_scope_filter_rejects_source_scope_mismatch() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = trimmed.split_once('=') {
                unsafe {
                    std::env::set_var(key.trim(), value.trim_matches('\"'));
                }
            }
        }
    }

    let cfg = AppConfig::from_env().expect("config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let (_workspace_code, _source_project_code, target_project_code, _transfer_policy_code) =
        create_stage2_import_shared_context(&client, suffix).await;
    let other_project_code = format!("memory_relation_edge_other_proj_{suffix}");
    let repo_root = format!("/tmp/{other_project_code}");
    std::fs::create_dir_all(&repo_root).expect("repo root");
    let other_project = upsert_project(
        &client,
        &other_project_code,
        "Memory Relation Edge Other Project",
        &repo_root,
        Some("main"),
        "default",
        "project_shared",
        "local_strict",
    )
    .await
    .expect("other project");
    ensure_namespace(
        &client,
        other_project.project_id,
        "default",
        Some("Default"),
        "local_strict",
    )
    .await
    .expect("other namespace");
    let provenance_other = json!({
        "source_event_ids": [format!("event:relation-edge-other:{suffix}")],
        "artifact_refs": [format!("artifact://proof/relation-edge-other/{suffix}")],
        "message_refs": [format!("thread:relation-edge-other:{suffix}")],
        "evidence_span": {"kind":"memory_card","suffix":suffix},
        "source_kind": "relation_card_seed",
    });
    let provenance_target = json!({
        "source_event_ids": [format!("event:relation-edge-target:{suffix}")],
        "artifact_refs": [format!("artifact://proof/relation-edge-target/{suffix}")],
        "message_refs": [format!("thread:relation-edge-target:{suffix}")],
        "evidence_span": {"kind":"memory_card","suffix":suffix},
        "source_kind": "relation_card_seed",
    });
    let left = create_memory_card(
        &client,
        &other_project_code,
        "default",
        &format!("relation left {suffix}"),
        "relation left",
        "relation left body",
        &[],
        &provenance_other,
        Some("other"),
        Some("relates_to"),
        Some("other"),
        Some("current"),
        Some("verified"),
        Some("active"),
        Some(1_000),
        Some(1_000),
        Some(1_000),
        None,
        Some(1_000),
    )
    .await
    .expect("other memory card");
    let right = create_memory_card(
        &client,
        &target_project_code,
        "default",
        &format!("relation right {suffix}"),
        "relation right",
        "relation right body",
        &[],
        &provenance_target,
        Some("target"),
        Some("relates_to"),
        Some("target"),
        Some("current"),
        Some("verified"),
        Some("active"),
        Some(1_000),
        Some(1_000),
        Some(1_000),
        None,
        Some(1_000),
    )
    .await
    .expect("target memory card");
    let error = create_memory_relation_edge(
        &client,
        &target_project_code,
        "default",
        left.memory_card_id,
        right.memory_card_id,
        "related_to",
        Some("active"),
        &json!({"proof":"stage2"}),
        Some("runtime_cli"),
        Some(&json!([format!(
            "event:memory-relation-edge-mismatch:{suffix}"
        )])),
        Some(&json!([format!(
            "artifact://proof/memory-relation-edge-mismatch/{suffix}"
        )])),
        Some(&json!([format!(
            "message:memory-relation-edge-mismatch:{suffix}"
        )])),
        Some(&json!({"kind":"memory_relation_edge"})),
        Some("extract"),
        Some("memory-relation-edge-envelope-v1"),
        Some(1_100),
        Some(1_100),
        None,
    )
    .await
    .expect_err("relation edge scope mismatch rejected");
    assert!(
        error
            .to_string()
            .contains("memory relation edge source memory card scope does not match")
    );
}

#[tokio::test]
async fn create_memory_relation_edge_verification_conflict_check_detects_poisoned_evidence_span() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = trimmed.split_once('=') {
                unsafe {
                    std::env::set_var(key.trim(), value.trim_matches('\"'));
                }
            }
        }
    }

    let cfg = AppConfig::from_env().expect("config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let (_workspace_code, _source_project_code, target_project_code, _transfer_policy_code) =
        create_stage2_import_shared_context(&client, suffix).await;
    let provenance_left = json!({
        "source_event_ids": [format!("event:relation-edge-left:{suffix}")],
        "artifact_refs": [format!("artifact://proof/relation-edge-left/{suffix}")],
        "message_refs": [format!("thread:relation-edge-left:{suffix}")],
        "evidence_span": {"kind":"memory_card","suffix":suffix},
        "source_kind": "relation_card_seed",
    });
    let provenance_right = json!({
        "source_event_ids": [format!("event:relation-edge-right:{suffix}")],
        "artifact_refs": [format!("artifact://proof/relation-edge-right/{suffix}")],
        "message_refs": [format!("thread:relation-edge-right:{suffix}")],
        "evidence_span": {"kind":"memory_card","suffix":suffix},
        "source_kind": "relation_card_seed",
    });
    let left = create_memory_card(
        &client,
        &target_project_code,
        "default",
        &format!("relation left {suffix}"),
        "relation left",
        "relation left body",
        &[],
        &provenance_left,
        Some("left"),
        Some("relates_to"),
        Some("left"),
        Some("current"),
        Some("verified"),
        Some("active"),
        Some(1_000),
        Some(1_000),
        Some(1_000),
        None,
        Some(1_000),
    )
    .await
    .expect("left memory card");
    let right = create_memory_card(
        &client,
        &target_project_code,
        "default",
        &format!("relation right {suffix}"),
        "relation right",
        "relation right body",
        &[],
        &provenance_right,
        Some("right"),
        Some("relates_to"),
        Some("right"),
        Some("current"),
        Some("verified"),
        Some("active"),
        Some(1_000),
        Some(1_000),
        Some(1_000),
        None,
        Some(1_000),
    )
    .await
    .expect("right memory card");
    let error = create_memory_relation_edge(
        &client,
        &target_project_code,
        "default",
        left.memory_card_id,
        right.memory_card_id,
        "related_to",
        Some("active"),
        &json!({"proof":"stage2"}),
        Some("runtime_cli"),
        Some(&json!([format!(
            "event:memory-relation-edge-poison:{suffix}"
        )])),
        Some(&json!([format!(
            "artifact://proof/memory-relation-edge-poison/{suffix}"
        )])),
        Some(&json!([format!(
            "message:memory-relation-edge-poison:{suffix}"
        )])),
        Some(&json!({"kind":"memory_relation_edge","poisoned":true})),
        Some("extract"),
        Some("memory-relation-edge-envelope-v1"),
        Some(1_100),
        Some(1_100),
        None,
    )
    .await
    .expect_err("poisoned relation edge rejected");
    assert!(
        error
            .to_string()
            .contains("memory relation edge evidence_span is flagged poisoned")
    );
}

#[test]
fn observability_payload_prefers_source_event_id_for_event_key() {
    let payload = json!({
        "token_budget_event": {
            "event_id": "event-123",
            "source_kind": "live_context_pack",
            "project_code": "amai",
            "namespace_code": "continuity",
            "created_at_epoch_ms": 1234
        }
    });
    let (stored, meta) =
        prepare_observability_payload("token_budget_event", &payload).expect("payload");
    assert_eq!(meta.event_key, "event-123");
    assert_eq!(meta.source_event_id.as_deref(), Some("event-123"));
    assert_eq!(meta.scope_project_code.as_deref(), Some("amai"));
    assert_eq!(meta.scope_namespace_code.as_deref(), Some("continuity"));
    assert_eq!(meta.captured_at_epoch_ms, Some(1234));
    assert_eq!(
        stored["_observability"]["replay_protected"].as_bool(),
        Some(true)
    );
}

#[test]
fn observability_payload_prefers_working_state_event_id_over_context_pack_id() {
    let payload = json!({
        "working_state_event": {
            "event_id": "working-state-event-1",
            "context_pack_id": "ctx-pack-1",
            "source_kind": "context_pack",
            "project": {
                "code": "amai"
            },
            "namespace": {
                "code": "default"
            },
            "recorded_at_epoch_ms": 555
        }
    });
    let (_stored, meta) =
        prepare_observability_payload("working_state_event", &payload).expect("payload");
    assert_eq!(meta.event_key, "working-state-event-1");
    assert_eq!(
        meta.source_event_id.as_deref(),
        Some("working-state-event-1")
    );
}

#[test]
fn observability_payload_prefers_explicit_observability_event_over_context_pack_id() {
    let payload = json!({
        "_observability": {
            "source_event_id": "benchmark-run-1",
            "source_kind": "benchmark_run",
            "captured_at_epoch_ms": 777
        },
        "benchmark": {
            "project": "project_alpha",
            "namespace": "default",
            "captured_at_epoch_ms": 777
        },
        "context_pack_id": "ctx-pack-1"
    });
    let (_stored, meta) =
        prepare_observability_payload("retrieval_benchmark_hot", &payload).expect("payload");
    assert_eq!(meta.event_key, "benchmark-run-1");
    assert_eq!(meta.source_event_id.as_deref(), Some("benchmark-run-1"));
    assert_eq!(meta.scope_project_code.as_deref(), Some("project_alpha"));
    assert_eq!(meta.scope_namespace_code.as_deref(), Some("default"));
    assert_eq!(meta.captured_at_epoch_ms, Some(777));
}

#[test]
fn observability_payload_extracts_scope_from_working_state_restore_root() {
    let payload = json!({
        "working_state_restore": {
            "project": {
                "code": "amai"
            },
            "namespace": {
                "code": "default"
            },
            "captured_at_epoch_ms": 999
        }
    });
    let (_stored, meta) =
        prepare_observability_payload("working_state_restore", &payload).expect("payload");
    assert_eq!(meta.scope_project_code.as_deref(), Some("amai"));
    assert_eq!(meta.scope_namespace_code.as_deref(), Some("default"));
    assert_eq!(meta.captured_at_epoch_ms, Some(999));
}

#[test]
fn observability_payload_marks_live_context_benchmark_as_contaminated() {
    let payload = json!({
        "load_verification": {
            "project": "amai",
            "namespace": "default",
            "captured_at_epoch_ms": 99,
            "record_live_context": true,
            "publish_benchmark_snapshot": false
        }
    });
    let (stored, meta) =
        prepare_observability_payload("retrieval_load_hot", &payload).expect("payload");
    assert_eq!(meta.source_class, "live_context");
    assert_eq!(
        stored["_observability"]["source_class"].as_str(),
        Some("live_context")
    );
}

#[test]
fn observability_source_class_defaults_to_benchmark_for_clean_load_snapshot() {
    let payload = json!({
        "load_verification": {
            "captured_at_epoch_ms": 77,
            "record_live_context": false,
            "publish_benchmark_snapshot": true
        }
    });
    assert_eq!(
        observability_source_class("retrieval_load_hot", &payload),
        "benchmark"
    );
    assert_eq!(
        observability_source_class("continuity_verification", &json!({})),
        "benchmark"
    );
}

#[test]
fn observability_payload_preserves_custom_meta_and_stamps_policy_versions() {
    let payload = json!({
        "_observability": {
            "benchmark_run_id": "bench-42"
        },
        "benchmark": {
            "project": "project_alpha",
            "namespace": "default",
            "captured_at_epoch_ms": 777
        }
    });
    let (stored, _meta) =
        prepare_observability_payload("retrieval_benchmark_hot", &payload).expect("payload");
    assert_eq!(
        stored["_observability"]["benchmark_run_id"].as_str(),
        Some("bench-42")
    );
    assert!(
        stored["_observability"]["schema_version"]
            .as_u64()
            .is_some()
    );
    assert_eq!(
        stored["_observability"]["classification_rules_version"].as_str(),
        Some("observability-source-class-v2")
    );
    assert_eq!(
        stored["_observability"]["immutable_snapshot"].as_bool(),
        Some(true)
    );
}

#[test]
fn observability_conflict_error_marks_newer_same_event_as_anti_replay() {
    let meta = ObservabilityInsertMeta {
        event_key: "event-1".to_string(),
        source_event_id: Some("event-1".to_string()),
        source_kind: "benchmark_run".to_string(),
        source_class: "benchmark".to_string(),
        scope_project_code: Some("project_alpha".to_string()),
        scope_namespace_code: Some("default".to_string()),
        captured_at_epoch_ms: Some(200),
        payload_sha256: "abc".to_string(),
    };
    let error = observability_conflict_error(
        "retrieval_benchmark_hot",
        &meta,
        Uuid::nil(),
        Some("event-1"),
        Some(100),
    );
    assert!(
        error
            .to_string()
            .contains("observability anti-replay blocked newer divergent payload")
    );
}

#[test]
fn observability_conflict_error_marks_divergent_payload_as_idempotency_failure() {
    let meta = ObservabilityInsertMeta {
        event_key: "event-2".to_string(),
        source_event_id: Some("event-2".to_string()),
        source_kind: "benchmark_run".to_string(),
        source_class: "benchmark".to_string(),
        scope_project_code: Some("project_alpha".to_string()),
        scope_namespace_code: Some("default".to_string()),
        captured_at_epoch_ms: Some(100),
        payload_sha256: "abc".to_string(),
    };
    let error = observability_conflict_error(
        "retrieval_benchmark_hot",
        &meta,
        Uuid::parse_str("00000000-0000-0000-0000-000000000123").expect("uuid"),
        Some("event-2"),
        Some(100),
    );
    assert!(
        error
            .to_string()
            .contains("observability idempotency blocked divergent payload")
    );
}

#[test]
fn immutable_observability_update_is_rejected_before_sql_write() {
    let snapshot_id = Uuid::parse_str("00000000-0000-0000-0000-000000000321").expect("uuid");
    let existing = json!({
        "_observability": {
            "immutable_snapshot": true
        },
        "benchmark": {
            "p95_ms": 1.0
        }
    });
    let incoming = json!({
        "_observability": {
            "immutable_snapshot": true
        },
        "benchmark": {
            "p95_ms": 2.0
        }
    });
    let error = validate_observability_update(
        "retrieval_benchmark_hot",
        &snapshot_id,
        &existing,
        &incoming,
    )
    .expect_err("immutable update must fail");
    assert!(
        error
            .to_string()
            .contains("observability snapshot is immutable and cannot be updated")
    );
}

#[test]
fn canonical_repo_root_string_resolves_relative_segments() {
    let temp_root =
        std::env::temp_dir().join(format!("amai-postgres-canonical-{}", Uuid::new_v4()));
    let nested = temp_root.join("nested");
    std::fs::create_dir_all(&nested).expect("temp dir");
    let raw = nested.join("..").join("nested").join(".");
    let canonical = canonical_repo_root_string(&raw.display().to_string()).expect("canonical");
    assert_eq!(canonical, nested.display().to_string());
    std::fs::remove_dir_all(&temp_root).expect("cleanup");
}

#[test]
fn canonical_repo_root_string_rejects_missing_paths() {
    let missing = std::env::temp_dir().join(format!("amai-postgres-missing-{}", Uuid::new_v4()));
    let error = canonical_repo_root_string(&missing.display().to_string())
        .expect_err("missing path must fail");
    assert!(error.to_string().contains("failed to resolve repo_root"));
}

#[test]
fn exact_match_basename_strips_parent_segments() {
    assert_eq!(
        exact_match_basename("docs/source/checklists/CHECKLIST_00_MASTER_ART_REGART"),
        "CHECKLIST_00_MASTER_ART_REGART"
    );
    assert_eq!(
        exact_match_basename("scripts/tools/amai_art_continuity_startup.sh"),
        "amai_art_continuity_startup.sh"
    );
}

#[test]
fn exact_match_basename_stem_strips_single_extension() {
    assert_eq!(
        exact_match_basename_stem("amai_art_continuity_startup.sh"),
        "amai_art_continuity_startup"
    );
    assert_eq!(
        exact_match_basename_stem("CHECKLIST_00_MASTER_ART_REGART"),
        "CHECKLIST_00_MASTER_ART_REGART"
    );
}

#[tokio::test]
async fn stack_meta_roundtrips_json_value() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = trimmed.split_once('=') {
                unsafe {
                    std::env::set_var(key.trim(), value.trim_matches('\"'));
                }
            }
        }
    }

    let cfg = AppConfig::from_env().expect("config");
    let client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let meta_key = format!("stage2_stack_meta_{suffix}");
    let meta_value = json!({
        "kind": "stage2_stack_meta",
        "suffix": suffix,
        "enabled": true,
    });

    upsert_stack_meta(&client, &meta_key, &meta_value)
        .await
        .expect("upsert stack meta");
    let loaded = get_stack_meta(&client, &meta_key)
        .await
        .expect("get stack meta");

    assert_eq!(loaded, Some(meta_value));
}

#[tokio::test]
async fn replace_document_index_upserts_single_document_and_preserves_namespace_count() {
    if let Ok(env_text) =
        std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
    {
        for line in env_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = trimmed.split_once('=') {
                unsafe {
                    std::env::set_var(key.trim(), value.trim_matches('\"'));
                }
            }
        }
    }

    let cfg = AppConfig::from_env().expect("config");
    let mut client = connect_admin(&cfg).await.expect("postgres");
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let namespace_code = format!("doc_index_{suffix}");
    let namespace = ensure_project_alpha_test_namespace(&client, &namespace_code).await;
    let project = get_project_by_code(&client, "project_alpha")
        .await
        .expect("project_alpha");
    let repo_root = format!("/tmp/postgres_doc_index_{suffix}");
    std::fs::create_dir_all(format!("{repo_root}/src")).expect("repo root");
    let absolute_path = format!("{repo_root}/src/lib.rs");

    let first_doc = DocumentRecord {
        project_id: project.project_id,
        namespace_id: namespace.namespace_id,
        repo_root: repo_root.clone(),
        absolute_path: absolute_path.clone(),
        relative_path: "src/lib.rs".to_string(),
        language: Some("rust".to_string()),
        source_kind: "git".to_string(),
        git_commit_sha: Some(format!("commit-{suffix}-1")),
        file_sha256: format!("{:064x}", suffix),
        line_count: 1,
        byte_count: 18,
        content: "pub fn first() {}\n".to_string(),
        metrics: json!({"bytes":18}),
        structure: json!({"items":1}),
        imports: json!([]),
        exports: json!(["first"]),
        diagnostics: json!([]),
        metadata: json!({"revision":1}),
    };
    let first_symbols = vec![SymbolRecord {
        name: "first".to_string(),
        kind: "function".to_string(),
        start_line: 1,
        end_line: 1,
        start_byte: 0,
        end_byte: 16,
        metadata: json!({"revision":1}),
    }];
    let first_chunks = vec![ChunkRecord {
        chunk_id: Uuid::new_v4(),
        qdrant_point_id: None,
        qdrant_collection_alias: None,
        chunk_index: 0,
        total_chunks: 1,
        start_line: 1,
        end_line: 1,
        start_byte: 0,
        end_byte: 16,
        content: "pub fn first() {}".to_string(),
        metadata: json!({"revision":1}),
    }];

    replace_document_index(&mut client, &first_doc, &first_symbols, &first_chunks)
        .await
        .expect("insert document");
    let first_count =
        count_documents_for_project_namespace_codes(&client, "project_alpha", &namespace_code)
            .await
            .expect("first count");
    assert_eq!(first_count, 1);

    let second_doc = DocumentRecord {
        git_commit_sha: Some(format!("commit-{suffix}-2")),
        file_sha256: format!("{:064x}", suffix + 1),
        line_count: 1,
        byte_count: 19,
        content: "pub fn second() {}\n".to_string(),
        metrics: json!({"bytes":19}),
        structure: json!({"items":1}),
        imports: json!([]),
        exports: json!(["second"]),
        diagnostics: json!([]),
        metadata: json!({"revision":2}),
        ..first_doc
    };
    let second_symbols = vec![SymbolRecord {
        name: "second".to_string(),
        kind: "function".to_string(),
        start_line: 1,
        end_line: 1,
        start_byte: 0,
        end_byte: 17,
        metadata: json!({"revision":2}),
    }];
    let second_chunks = vec![ChunkRecord {
        chunk_id: Uuid::new_v4(),
        qdrant_point_id: None,
        qdrant_collection_alias: None,
        chunk_index: 0,
        total_chunks: 1,
        start_line: 1,
        end_line: 1,
        start_byte: 0,
        end_byte: 17,
        content: "pub fn second() {}".to_string(),
        metadata: json!({"revision":2}),
    }];

    replace_document_index(&mut client, &second_doc, &second_symbols, &second_chunks)
        .await
        .expect("upsert document");
    let second_count =
        count_documents_for_project_namespace_codes(&client, "project_alpha", &namespace_code)
            .await
            .expect("second count");
    assert_eq!(second_count, 1);
}

#[test]
fn safe_postgres_descriptor_masks_password_for_uri_dsn() {
    let masked = safe_postgres_descriptor(
        "postgres://art_user:super-secret@example.com:5544/amai?sslmode=require",
    );
    assert_eq!(
        masked,
        "postgres://art_user:***@example.com:5544/amai?sslmode=require"
    );
    assert!(!masked.contains("super-secret"));
}

#[test]
fn safe_postgres_descriptor_masks_password_for_keyword_dsn() {
    let masked = safe_postgres_descriptor(
        "host=pg.internal port=5433 user=app dbname=amai password=very-secret sslmode=prefer",
    );
    assert_eq!(
        masked,
        "postgres://app:***@pg.internal:5433/amai?sslmode=prefer"
    );
    assert!(!masked.contains("very-secret"));
}

#[test]
fn bootstrap_schema_cache_roundtrips() {
    let cache_key = format!("test-bootstrap-cache-{}", Uuid::new_v4());
    assert!(!super::bootstrap_schema_cache_contains(&cache_key));
    super::bootstrap_schema_cache_insert(cache_key.clone());
    assert!(super::bootstrap_schema_cache_contains(&cache_key));
}
