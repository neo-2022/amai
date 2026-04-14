use super::*;

pub(super) fn memory_item_record_from_row(row: &Row) -> MemoryItemRecord {
    MemoryItemRecord {
        memory_item_id: row.get(0),
        workspace_code: row.get(1),
        project_code: row.get(2),
        namespace_code: row.get(3),
        source_project_code: row.get(4),
        import_packet_id: row.get(5),
        owner_agent_id: row.get(6),
        visibility_scope: row.get(7),
        item_kind: row.get(8),
        identity_key: row.get(9),
        title: row.get(10),
        summary: row.get(11),
        body: row.get(12),
        sensitivity_class: row.get(13),
        truth_state: row.get(14),
        trust_state: row.get(15),
        verification_state: row.get(16),
        lifecycle_state: row.get(17),
        source_event_ids: row.get(18),
        artifact_refs: row.get(19),
        message_refs: row.get(20),
        evidence_span: row.get(21),
        derivation_kind: row.get(22),
        observed_at_epoch_ms: row.get(23),
        recorded_at_epoch_ms: row.get(24),
        valid_from_epoch_ms: row.get(25),
        valid_to_epoch_ms: row.get(26),
        last_verified_at_epoch_ms: row.get(27),
        ingest_seq: row.get(28),
        object_version: row.get(29),
        causation_id: row.get(30),
        correlation_id: row.get(31),
        utility_score: row.get(32),
        freshness_score: row.get(33),
        retention_class: row.get(34),
        ttl_epoch_ms: row.get(35),
        access_count: row.get(36),
        last_accessed_at: maybe_rfc3339_utc(row, 37),
        decay_policy: row.get(38),
        consolidation_status: row.get(39),
        imported_from: row.get(40),
        schema_version: row.get(41),
        superseded_by_memory_item_id: row.get(42),
        metadata: row.get(43),
    }
}

pub(super) fn build_memory_write_pipeline(
    memory_item: &MemoryItemRecord,
    candidate: &MemoryItemCandidateExtraction,
    scope_filter: &MemoryItemPolicyScopeFilter,
    verification_check: &MemoryItemVerificationConflictCheck,
    raw_memory_event_id: Uuid,
    async_index_subjects: &[&str],
    fan_out_subjects: &[&str],
) -> Value {
    json!({
        "contract_version": "memory-write-pipeline-v1",
        "raw_event_append": {
            "status": "written",
            "source_basis_status": candidate.source_basis_status,
            "storage_lane": "ami.memory_raw_events",
            "memory_raw_event_id": raw_memory_event_id,
            "source_event_count": candidate.source_event_count,
            "artifact_ref_count": candidate.artifact_ref_count,
            "message_ref_count": candidate.message_ref_count,
            "has_evidence_span": candidate.has_evidence_span,
        },
        "memory_candidate_extraction": {
            "status": "materialized",
            "item_kind": memory_item.item_kind,
            "derivation_kind": memory_item.derivation_kind,
            "candidate_class": candidate.candidate_class,
            "source_kind": candidate.source_kind,
            "hot_path_write_eligible": candidate.hot_path_write_eligible,
            "background_consolidation_recommended": candidate.background_consolidation_recommended,
        },
        "policy_and_scope_filter": {
            "status": "applied",
            "visibility_scope": scope_filter.visibility_scope,
            "sensitivity_class": scope_filter.sensitivity_class,
            "workspace_code": scope_filter.workspace_code,
            "project_code": scope_filter.project_code,
            "namespace_code": scope_filter.namespace_code,
            "owner_agent_required": scope_filter.owner_agent_required,
            "owner_agent_present": scope_filter.owner_agent_present,
            "private_contour_violation": scope_filter.private_contour_violation,
            "cross_project_basis_present": scope_filter.cross_project_basis_present,
            "source_project_bound": scope_filter.source_project_bound,
            "scope_allowed": scope_filter.scope_allowed,
        },
        "verification_conflict_check": {
            "status": "applied",
            "truth_state": verification_check.truth_state,
            "trust_state": verification_check.trust_state,
            "verification_state": verification_check.verification_state,
            "superseded_by_memory_item_id": verification_check.superseded_by_memory_item_id,
            "evidence_present": verification_check.evidence_present,
            "current_truth_conflict": verification_check.current_truth_conflict,
            "poisoned_detected": verification_check.poisoned_detected,
            "private_contour_violation": verification_check.private_contour_violation,
            "write_allowed": verification_check.write_allowed,
        },
        "truth_write": {
            "status": "written",
            "storage_lane": "ami.memory_items",
            "memory_item_id": memory_item.memory_item_id,
            "object_version": memory_item.object_version,
            "ingest_seq": memory_item.ingest_seq,
        },
        "async_indexing": {
            "status": "queued",
            "storage_lane": "ami.memory_write_outbox",
            "expected_targets": ["lexical", "graph", "embedding", "restore_summary"],
            "queued_subjects": async_index_subjects,
        },
        "cache_invalidation_fan_out": {
            "status": "queued",
            "storage_lane": "ami.memory_write_outbox",
            "event_plane": "nats_or_compatible",
            "queued_subjects": fan_out_subjects,
        }
    })
}

pub(super) fn memory_write_async_index_subjects() -> [&'static str; 4] {
    [
        "ami.index.memory.lexical",
        "ami.index.memory.graph",
        "ami.index.memory.embedding",
        "ami.index.memory.restore_summary",
    ]
}

pub(super) fn memory_write_fan_out_subjects() -> [&'static str; 2] {
    [
        "ami.event.memory_item.created",
        "ami.event.memory_item.invalidate_cache",
    ]
}

fn classify_memory_item_candidate(item_kind: &str) -> String {
    canonical_candidate_class_from_hints(None, Some(item_kind), None, &[], false, "fact")
}

pub(super) fn derive_memory_item_source_kind(record: &MemoryItemInsert<'_>) -> Option<String> {
    if let Some(source_kind) = record.metadata.get("source_kind").and_then(Value::as_str) {
        let trimmed = source_kind.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    if record.import_packet_id.is_some() || record.source_project_code.is_some() {
        return Some("import_packet_basis".to_string());
    }
    if !record.source_event_ids.is_empty() {
        return Some("raw_event_append".to_string());
    }
    if !record.artifact_refs.is_empty() {
        return Some("artifact_capture".to_string());
    }
    if !record.message_refs.is_empty() {
        return Some("message_capture".to_string());
    }
    if record.evidence_span != &json!({}) {
        return Some("evidence_span_capture".to_string());
    }
    None
}

pub(super) fn memory_item_has_recorded_basis(record: &MemoryItemInsert<'_>) -> bool {
    !record.source_event_ids.is_empty()
        || !record.artifact_refs.is_empty()
        || !record.message_refs.is_empty()
        || record.evidence_span != &json!({})
}

pub(super) fn extract_memory_item_candidate(
    workspace_id: Uuid,
    project: &ProjectRecord,
    namespace: &NamespaceRecord,
    source_project_id: Option<Uuid>,
    owner_agent_id: Option<Uuid>,
    record: &MemoryItemInsert<'_>,
    observed_at_epoch_ms: Option<i64>,
    recorded_at_epoch_ms: Option<i64>,
    valid_from_epoch_ms: Option<i64>,
    valid_to_epoch_ms: Option<i64>,
    source_event_ids: &Value,
    artifact_refs: &Value,
    message_refs: &Value,
) -> MemoryItemCandidateExtraction {
    let source_event_count = record.source_event_ids.len();
    let artifact_ref_count = record.artifact_refs.len();
    let message_ref_count = record.message_refs.len();
    let has_evidence_span = record.evidence_span != &json!({});
    let source_kind = derive_memory_item_source_kind(record);
    let imported_from = record.imported_from.cloned().unwrap_or_else(|| {
        if record.source_project_code.is_some() || record.import_packet_id.is_some() {
            json!({
                "source_project_code": record.source_project_code,
                "import_packet_id": record.import_packet_id,
            })
        } else {
            json!({})
        }
    });
    let candidate_class = classify_memory_item_candidate(record.item_kind);
    let hot_path_write_eligible = record.derivation_kind == Some("operator_write")
        || matches!(candidate_class.as_str(), "commitment" | "decision");
    let background_consolidation_recommended = !hot_path_write_eligible
        && matches!(
            candidate_class.as_str(),
            "fact" | "skill_hint" | "artifact_ref"
        );
    let source_basis_status =
        if record.import_packet_id.is_some() || record.source_project_code.is_some() {
            "import_packet_basis".to_string()
        } else if source_event_count > 0
            || artifact_ref_count > 0
            || message_ref_count > 0
            || has_evidence_span
        {
            "recorded".to_string()
        } else {
            "operator_only".to_string()
        };
    let raw_event_kind =
        if record.import_packet_id.is_some() || record.source_project_code.is_some() {
            "memory_candidate_import".to_string()
        } else if record.derivation_kind == Some("verified_write_back") {
            "memory_candidate_write_back".to_string()
        } else {
            "memory_candidate_write".to_string()
        };
    let raw_event_payload = json!({
        "candidate": {
            "item_kind": record.item_kind,
            "candidate_class": candidate_class,
            "identity_key": record.identity_key,
            "title": record.title,
            "summary": record.summary,
            "body_present": record.body.is_some(),
            "derivation_kind": record.derivation_kind,
        },
        "runtime_lane": {
            "source_basis_status": source_basis_status,
            "source_kind": source_kind,
            "hot_path_write_eligible": hot_path_write_eligible,
            "background_consolidation_recommended": background_consolidation_recommended,
        },
        "scope": {
            "workspace_id": workspace_id,
            "project_id": project.project_id,
            "project_code": project.code,
            "namespace_id": namespace.namespace_id,
            "namespace_code": namespace.code,
            "visibility_scope": project.visibility_scope,
            "source_project_id": source_project_id,
            "source_project_code": record.source_project_code,
            "import_packet_id": record.import_packet_id,
            "owner_agent_id": owner_agent_id,
        },
        "evidence": {
            "source_event_ids": source_event_ids,
            "artifact_refs": artifact_refs,
            "message_refs": message_refs,
            "evidence_span": record.evidence_span,
        },
        "truth": {
            "truth_state": record.truth_state.unwrap_or("proposed"),
            "trust_state": record.trust_state.unwrap_or("proposed"),
            "verification_state": record.verification_state.unwrap_or("unverified"),
            "lifecycle_state": record.lifecycle_state.unwrap_or("hot"),
        },
        "timing": {
            "observed_at_epoch_ms": observed_at_epoch_ms,
            "recorded_at_epoch_ms": recorded_at_epoch_ms,
            "valid_from_epoch_ms": valid_from_epoch_ms,
            "valid_to_epoch_ms": valid_to_epoch_ms,
        },
        "metadata": record.metadata,
    });
    MemoryItemCandidateExtraction {
        source_basis_status,
        source_event_count,
        artifact_ref_count,
        message_ref_count,
        has_evidence_span,
        source_kind,
        imported_from,
        raw_event_kind,
        raw_event_payload,
        candidate_class,
        hot_path_write_eligible,
        background_consolidation_recommended,
    }
}

pub(super) fn run_memory_item_policy_scope_filter(
    workspace_code: &str,
    project: &ProjectRecord,
    namespace: &NamespaceRecord,
    owner_agent_id: Option<Uuid>,
    record: &MemoryItemInsert<'_>,
) -> MemoryItemPolicyScopeFilter {
    let visibility_scope = project.visibility_scope.clone();
    let sensitivity_class = record.sensitivity_class.unwrap_or("internal").to_string();
    let owner_agent_required = visibility_scope == "agent_private";
    let owner_agent_present = owner_agent_id.is_some();
    let private_contour_violation = owner_agent_required && !owner_agent_present;
    let quarantine_contour_violation = visibility_scope == "quarantine";
    let cross_project_basis_present =
        record.source_project_code.is_some() || record.import_packet_id.is_some();
    let source_project_bound = record.source_project_code.is_some();
    let controlled_transfer_required = cross_project_basis_present;
    let scope_allowed = !private_contour_violation
        && !quarantine_contour_violation
        && !controlled_transfer_required;
    MemoryItemPolicyScopeFilter {
        visibility_scope,
        sensitivity_class,
        workspace_code: workspace_code.to_string(),
        project_code: project.code.clone(),
        namespace_code: Some(namespace.code.clone()),
        owner_agent_required,
        owner_agent_present,
        private_contour_violation,
        quarantine_contour_violation,
        cross_project_basis_present,
        source_project_bound,
        import_packet_present: false,
        import_packet_found: false,
        import_packet_source_matches: true,
        import_packet_target_matches: true,
        import_packet_status: None,
        controlled_transfer_required,
        controlled_transfer_valid: !controlled_transfer_required,
        scope_allowed,
    }
}

async fn hydrate_memory_item_policy_scope_filter(
    client: &Client,
    project: &ProjectRecord,
    record: &MemoryItemInsert<'_>,
    base_filter: MemoryItemPolicyScopeFilter,
) -> MemoryItemPolicyScopeFilter {
    let mut filter = base_filter;
    filter.import_packet_present = record.import_packet_id.is_some();
    if !filter.controlled_transfer_required {
        filter.controlled_transfer_valid = true;
        filter.scope_allowed =
            !filter.private_contour_violation && !filter.quarantine_contour_violation;
        return filter;
    }
    let Some(import_packet_id) = record.import_packet_id else {
        filter.controlled_transfer_valid = false;
        filter.scope_allowed = false;
        return filter;
    };
    match get_import_packet(client, import_packet_id).await {
        Ok(packet) => {
            filter.import_packet_found = true;
            filter.import_packet_status = Some(packet.status.clone());
            filter.import_packet_source_matches = record
                .source_project_code
                .is_none_or(|code| code == packet.source_project_code);
            filter.import_packet_target_matches = packet.target_project_code == project.code;
            filter.controlled_transfer_valid = filter.import_packet_source_matches
                && filter.import_packet_target_matches
                && matches!(
                    packet.status.as_str(),
                    "imported" | "borrowed_unverified" | "verified"
                );
        }
        Err(_) => {
            filter.import_packet_found = false;
            filter.controlled_transfer_valid = false;
        }
    }
    filter.scope_allowed = !filter.private_contour_violation
        && !filter.quarantine_contour_violation
        && filter.controlled_transfer_valid;
    filter
}

pub(super) fn validate_memory_item_policy_scope_filter(
    filter: &MemoryItemPolicyScopeFilter,
) -> Result<()> {
    if filter.quarantine_contour_violation {
        return Err(anyhow!(
            "memory candidate violates scope filter: visibility_scope=quarantine requires dedicated quarantine_item path"
        ));
    }
    if filter.controlled_transfer_required && !filter.import_packet_present {
        return Err(anyhow!(
            "memory candidate with cross-project basis requires controlled import_packet"
        ));
    }
    if filter.import_packet_present && !filter.import_packet_found {
        return Err(anyhow!("memory candidate references missing import_packet"));
    }
    if !filter.import_packet_source_matches {
        return Err(anyhow!(
            "memory candidate import_packet source project does not match source_project_code"
        ));
    }
    if !filter.import_packet_target_matches {
        return Err(anyhow!(
            "memory candidate import_packet target project does not match target contour"
        ));
    }
    if filter.controlled_transfer_required && !filter.controlled_transfer_valid {
        return Err(anyhow!(
            "memory candidate import_packet status {:?} does not allow controlled transfer",
            filter.import_packet_status
        ));
    }
    if !filter.scope_allowed {
        return Err(anyhow!(
            "memory candidate violates scope filter: visibility_scope={} requires owner_agent binding",
            filter.visibility_scope
        ));
    }
    Ok(())
}

pub(super) fn metadata_marks_memory_item_poisoned(metadata: &Value) -> bool {
    metadata
        .get("poisoned")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || metadata
            .get("safety")
            .and_then(Value::as_object)
            .and_then(|safety| safety.get("poisoned"))
            .and_then(Value::as_bool)
            .unwrap_or(false)
}

async fn run_memory_item_verification_conflict_check(
    client: &Client,
    project: &ProjectRecord,
    namespace: &NamespaceRecord,
    record: &MemoryItemInsert<'_>,
    candidate: &MemoryItemCandidateExtraction,
    scope_filter: &MemoryItemPolicyScopeFilter,
) -> Result<MemoryItemVerificationConflictCheck> {
    let evidence_present = record.derivation_kind == Some("operator_write")
        || candidate.source_basis_status != "operator_only";
    let poisoned_detected = metadata_marks_memory_item_poisoned(record.metadata);
    let current_truth_conflict =
        if record.truth_state == Some("current") && record.identity_key.is_some() {
            let row = client
                .query_one(
                    r#"
                    SELECT EXISTS(
                        SELECT 1
                        FROM ami.memory_items mi
                        WHERE mi.project_id = $1
                          AND mi.namespace_id = $2
                          AND mi.identity_key = $3
                          AND mi.truth_state = 'current'
                          AND mi.superseded_by_memory_item_id IS NULL
                    )
                    "#,
                    &[
                        &project.project_id,
                        &namespace.namespace_id,
                        &record.identity_key,
                    ],
                )
                .await
                .context("failed to check memory item current-truth conflict")?;
            row.get::<_, bool>(0)
        } else {
            false
        };
    let truth_state = record.truth_state.unwrap_or("proposed").to_string();
    let trust_state = record.trust_state.unwrap_or("proposed").to_string();
    let verification_state = record
        .verification_state
        .unwrap_or("unverified")
        .to_string();
    let write_allowed = evidence_present
        && !poisoned_detected
        && !current_truth_conflict
        && !scope_filter.private_contour_violation;
    Ok(MemoryItemVerificationConflictCheck {
        evidence_present,
        current_truth_conflict,
        poisoned_detected,
        private_contour_violation: scope_filter.private_contour_violation,
        truth_state,
        trust_state,
        verification_state,
        superseded_by_memory_item_id: record.superseded_by_memory_item_id,
        write_allowed,
    })
}

pub(super) fn validate_memory_item_verification_conflict_check(
    check: &MemoryItemVerificationConflictCheck,
) -> Result<()> {
    if check.poisoned_detected {
        return Err(anyhow!(
            "memory candidate is flagged poisoned by metadata and cannot be written"
        ));
    }
    if check.current_truth_conflict {
        return Err(anyhow!(
            "memory candidate conflicts with existing current truth for the same identity_key"
        ));
    }
    if !check.evidence_present {
        return Err(anyhow!(
            "memory candidate must carry evidence unless written as operator hot-path override"
        ));
    }
    if !check.write_allowed {
        return Err(anyhow!(
            "memory candidate failed verification/conflict check before truth write"
        ));
    }
    Ok(())
}

pub(super) fn validate_memory_item_candidate(
    record: &MemoryItemInsert<'_>,
    candidate: &MemoryItemCandidateExtraction,
) -> Result<()> {
    if record.derivation_kind == Some("verified_write_back") {
        if record.truth_state != Some("current")
            || record.trust_state != Some("verified")
            || record.verification_state != Some("verified")
        {
            return Err(anyhow!(
                "verified_write_back requires current/verified/verified truth, trust, and verification states"
            ));
        }
        if candidate.source_basis_status == "operator_only" {
            return Err(anyhow!(
                "verified_write_back requires recorded raw/artifact/message/evidence basis"
            ));
        }
    }
    if record.derivation_kind != Some("operator_write")
        && candidate.source_basis_status == "operator_only"
    {
        return Err(anyhow!(
            "memory candidate requires recorded basis unless derivation_kind=operator_write"
        ));
    }
    Ok(())
}

pub(super) fn augment_memory_item_metadata_with_stage2_runtime(
    metadata: &Value,
    candidate: &MemoryItemCandidateExtraction,
) -> Value {
    let mut object = match metadata {
        Value::Object(map) => map.clone(),
        _ => {
            let mut fallback = serde_json::Map::new();
            fallback.insert("user_metadata".to_string(), metadata.clone());
            fallback
        }
    };
    object.insert(
        "stage2_runtime".to_string(),
        json!({
            "candidate_class": candidate.candidate_class,
            "source_kind": candidate.source_kind,
            "source_basis_status": candidate.source_basis_status,
            "hot_path_write_eligible": candidate.hot_path_write_eligible,
            "background_consolidation_recommended": candidate.background_consolidation_recommended,
        }),
    );
    Value::Object(object)
}

fn task_node_record_from_row(row: &Row) -> TaskNodeRecord {
    TaskNodeRecord {
        task_node_id: row.get(0),
        workspace_code: row.get(1),
        project_code: row.get(2),
        namespace_code: row.get(3),
        parent_task_node_id: row.get(4),
        memory_item_id: row.get(5),
        task_key: row.get(6),
        task_role: row.get(7),
        headline: row.get(8),
        summary: row.get(9),
        next_step: row.get(10),
        execution_state: row.get(11),
        lifecycle_state: row.get(12),
        confidence: row.get(13),
        current_score: row.get(14),
        reopened_count: row.get(15),
        child_count: row.get(16),
        closed_child_count: row.get(17),
        pending_return_count: row.get(18),
        source_event_ids: row.get(19),
        artifact_refs: row.get(20),
        evidence_span: row.get(21),
        candidate_class: row.get(22),
        derivation_kind: row.get(23),
        source_kind: row.get(24),
        hot_path_write_eligible: row.get(25),
        background_consolidation_recommended: row.get(26),
        status_payload: row.get(27),
        metadata: row.get(28),
        opened_at_epoch_ms: row.get(29),
        closed_at_epoch_ms: row.get(30),
        archived_at_epoch_ms: row.get(31),
    }
}

fn task_event_record_from_row(row: &Row) -> TaskEventRecord {
    TaskEventRecord {
        task_event_id: row.get(0),
        workspace_code: row.get(1),
        project_code: row.get(2),
        namespace_code: row.get(3),
        task_node_id: row.get(4),
        source_snapshot_id: row.get(5),
        source_event_id: row.get(6),
        event_kind: row.get(7),
        prior_execution_state: row.get(8),
        next_execution_state: row.get(9),
        prior_lifecycle_state: row.get(10),
        next_lifecycle_state: row.get(11),
        source_kind: row.get(12),
        artifact_refs: row.get(13),
        message_refs: row.get(14),
        evidence_span: row.get(15),
        derivation_kind: row.get(16),
        schema_version: row.get(17),
        event_payload: row.get(18),
        recorded_at_epoch_ms: row.get(19),
    }
}

fn memory_link_decision_record_from_row(row: &Row) -> MemoryLinkDecisionRecord {
    MemoryLinkDecisionRecord {
        memory_link_decision_id: row.get(0),
        workspace_code: row.get(1),
        project_code: row.get(2),
        namespace_code: row.get(3),
        task_node_id: row.get(4),
        retrieval_trace_id: row.get(5),
        candidate_task_node_id: row.get(6),
        decision_outcome: row.get(7),
        legality_passed: row.get(8),
        scope_filter_passed: row.get(9),
        evidence_sufficient: row.get(10),
        classifier_label: row.get(11),
        classifier_score: row.get(12),
        decision_reason: row.get(13),
        decision_payload: row.get(14),
        source_event_ids: row.get(15),
        artifact_refs: row.get(16),
        message_refs: row.get(17),
        evidence_span: row.get(18),
        derivation_kind: row.get(19),
        schema_version: row.get(20),
        recorded_at_epoch_ms: row.get(21),
    }
}

fn pending_link_proposal_record_from_row(row: &Row) -> PendingLinkProposalRecord {
    PendingLinkProposalRecord {
        pending_link_proposal_id: row.get(0),
        workspace_code: row.get(1),
        project_code: row.get(2),
        namespace_code: row.get(3),
        task_node_id: row.get(4),
        retrieval_trace_id: row.get(5),
        candidate_task_node_id: row.get(6),
        proposal_state: row.get(7),
        proposal_reason: row.get(8),
        evidence_request: row.get(9),
        evidence_payload: row.get(10),
        classifier_score: row.get(11),
        ttl_epoch_ms: row.get(12),
        source_event_ids: row.get(13),
        artifact_refs: row.get(14),
        message_refs: row.get(15),
        evidence_span: row.get(16),
        derivation_kind: row.get(17),
        schema_version: row.get(18),
    }
}

pub(super) fn raw_evidence_record_from_row(row: &Row) -> RawEvidenceRecord {
    RawEvidenceRecord {
        memory_item_id: row.get(0),
        memory_provenance_id: row.get(1),
        project_code: row.get(2),
        namespace_code: row.get(3),
        title: row.get(4),
        summary: row.get(5),
        content: row.get(6),
        source_kind: row.get(7),
        source_event_id: row.get(8),
        artifact_refs: row.get(9),
        message_refs: row.get(10),
        evidence_span: row.get(11),
        details: row.get(12),
        derivation_kind: row.get(13),
        truth_state: row.get(14),
        trust_state: row.get(15),
        verification_state: row.get(16),
        observed_at_epoch_ms: row.get(17),
        recorded_at_epoch_ms: row.get(18),
        valid_from_epoch_ms: row.get(19),
        valid_to_epoch_ms: row.get(20),
        last_verified_at_epoch_ms: row.get(21),
    }
}

pub(super) fn memory_provenance_record_from_row(row: &Row) -> MemoryProvenanceRecord {
    MemoryProvenanceRecord {
        memory_provenance_id: row.get(0),
        workspace_code: row.get(1),
        project_code: row.get(2),
        namespace_code: row.get(3),
        memory_item_id: row.get(4),
        source_kind: row.get(5),
        source_event_id: row.get(6),
        source_snapshot_id: row.get(7),
        artifact_ref_id: row.get(8),
        trust_level: row.get(9),
        message_refs: row.get(10),
        evidence_span: row.get(11),
        derivation_kind: row.get(12),
        observed_at_epoch_ms: row.get(13),
        recorded_at_epoch_ms: row.get(14),
        valid_from_epoch_ms: row.get(15),
        valid_to_epoch_ms: row.get(16),
        schema_version: row.get(17),
        details: row.get(18),
    }
}

fn normalize_temporal_bounds(
    valid_from_epoch_ms: Option<i64>,
    valid_to_epoch_ms: Option<i64>,
) -> (Option<i64>, Option<i64>) {
    match (valid_from_epoch_ms, valid_to_epoch_ms) {
        (Some(valid_from), Some(valid_to)) if valid_to < valid_from => {
            (Some(valid_from), Some(valid_from))
        }
        _ => (valid_from_epoch_ms, valid_to_epoch_ms),
    }
}

pub async fn get_workspace_id_for_project(client: &Client, project_id: Uuid) -> Result<Uuid> {
    let row = client
        .query_one(
            "SELECT workspace_id FROM ami.projects WHERE project_id = $1",
            &[&project_id],
        )
        .await
        .with_context(|| format!("failed to resolve workspace_id for project {project_id}"))?;
    Ok(row.get(0))
}

pub(super) async fn resolve_scope_ids(
    client: &Client,
    project_code: &str,
    namespace_code: &str,
) -> Result<(Uuid, ProjectRecord, NamespaceRecord)> {
    let project = get_project_by_code(client, project_code).await?;
    let namespace = get_namespace_by_code(client, project.project_id, namespace_code).await?;
    let workspace_id = get_workspace_id_for_project(client, project.project_id).await?;
    Ok((workspace_id, project, namespace))
}

async fn fetch_memory_card_state(client: &Client, memory_card_id: Uuid) -> Result<MemoryCardState> {
    let row = client
        .query_one(
            r#"
            SELECT
                mc.project_id,
                mc.namespace_id,
                mc.truth_state,
                mc.verification_state,
                mc.status,
                mc.valid_from_epoch_ms,
                mc.valid_to_epoch_ms
            FROM ami.memory_cards mc
            WHERE mc.memory_card_id = $1
            "#,
            &[&memory_card_id],
        )
        .await
        .with_context(|| format!("failed to load memory card state {memory_card_id}"))?;
    Ok(MemoryCardState {
        project_id: row.get(0),
        namespace_id: row.get(1),
        truth_state: row.get(2),
        verification_state: row.get(3),
        status: row.get(4),
        valid_from_epoch_ms: row.get(5),
        valid_to_epoch_ms: row.get(6),
    })
}

async fn record_memory_card_transition(
    client: &Client,
    project_id: Uuid,
    namespace_id: Uuid,
    memory_card_id: Uuid,
    from_truth_state: Option<&str>,
    to_truth_state: Option<&str>,
    from_verification_state: Option<&str>,
    to_verification_state: Option<&str>,
    from_status: Option<&str>,
    to_status: Option<&str>,
    transition_reason: Option<&str>,
    transition_source: Option<&str>,
    recorded_at_epoch_ms: Option<i64>,
    effective_at_epoch_ms: Option<i64>,
) -> Result<()> {
    let now_epoch_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock before unix epoch")?
        .as_millis() as i64;
    let recorded_at_epoch_ms = recorded_at_epoch_ms.or(Some(now_epoch_ms));
    client
        .execute(
            r#"
            INSERT INTO ami.memory_card_transitions(
                project_id,
                namespace_id,
                memory_card_id,
                from_truth_state,
                to_truth_state,
                from_verification_state,
                to_verification_state,
                from_status,
                to_status,
                transition_reason,
                transition_source,
                recorded_at_epoch_ms,
                effective_at_epoch_ms
            )
            VALUES (
                $1, $2, $3, $4, $5, $6, $7,
                $8, $9, $10, $11, $12, $13
            )
            "#,
            &[
                &project_id,
                &namespace_id,
                &memory_card_id,
                &from_truth_state,
                &to_truth_state,
                &from_verification_state,
                &to_verification_state,
                &from_status,
                &to_status,
                &transition_reason,
                &transition_source,
                &recorded_at_epoch_ms,
                &effective_at_epoch_ms,
            ],
        )
        .await
        .with_context(|| format!("failed to record memory card transition {memory_card_id}"))?;
    Ok(())
}

pub async fn create_memory_card(
    client: &Client,
    project_code: &str,
    namespace_code: &str,
    title: &str,
    summary: &str,
    body: &str,
    tags: &[String],
    provenance: &Value,
    fact_subject: Option<&str>,
    fact_predicate: Option<&str>,
    fact_object: Option<&str>,
    truth_state: Option<&str>,
    verification_state: Option<&str>,
    status: Option<&str>,
    observed_at_epoch_ms: Option<i64>,
    recorded_at_epoch_ms: Option<i64>,
    valid_from_epoch_ms: Option<i64>,
    valid_to_epoch_ms: Option<i64>,
    last_verified_at_epoch_ms: Option<i64>,
) -> Result<MemoryCardRecord> {
    let project = get_project_by_code(client, project_code).await?;
    let namespace = get_namespace_by_code(client, project.project_id, namespace_code).await?;
    let candidate = extract_memory_card_candidate(
        title,
        tags,
        provenance,
        fact_subject,
        fact_predicate,
        fact_object,
    );
    validate_memory_card_candidate(&candidate)?;
    let scope_filter = run_memory_card_policy_scope_filter(&project, &namespace, provenance);
    validate_memory_card_policy_scope_filter(&scope_filter)?;
    let verification_check = run_memory_card_verification_conflict_check(
        client,
        &project,
        &namespace,
        &candidate,
        provenance,
        fact_subject,
        fact_predicate,
        fact_object,
        truth_state,
        verification_state,
        status,
        &scope_filter,
    )
    .await?;
    validate_memory_card_verification_conflict_check(&verification_check)?;
    validate_memory_card_runtime_states(
        truth_state,
        verification_state,
        status,
        "memory create-card",
    )?;
    let stored_provenance = augment_memory_card_provenance_with_stage2_preflight(
        provenance,
        &candidate,
        &scope_filter,
        &verification_check,
    );
    let now_epoch_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock before unix epoch")?
        .as_millis() as i64;
    let observed_at_epoch_ms = observed_at_epoch_ms.or(Some(now_epoch_ms));
    let recorded_at_epoch_ms = recorded_at_epoch_ms.or(Some(now_epoch_ms));
    let valid_from_epoch_ms = valid_from_epoch_ms
        .or(observed_at_epoch_ms)
        .or(recorded_at_epoch_ms);
    let (valid_from_epoch_ms, valid_to_epoch_ms) =
        normalize_temporal_bounds(valid_from_epoch_ms, valid_to_epoch_ms);
    let row = client
        .query_one(
            r#"
            INSERT INTO ami.memory_cards(
                project_id,
                namespace_id,
                title,
                summary,
                body,
                tags,
                provenance,
                fact_subject,
                fact_predicate,
                fact_object,
                truth_state,
                verification_state,
                status,
                derivation_kind,
                candidate_class,
                source_kind,
                hot_path_write_eligible,
                background_consolidation_recommended,
                observed_at_epoch_ms,
                recorded_at_epoch_ms,
                valid_from_epoch_ms,
                valid_to_epoch_ms,
                last_verified_at_epoch_ms
            )
            VALUES (
                $1, $2, $3, $4, $5,
                $6::jsonb, $7::jsonb,
                $8, $9, $10,
                COALESCE($11, 'current'),
                COALESCE($12, 'raw'),
                COALESCE($13, 'active'),
                $14,
                $15,
                $16,
                $17,
                $18,
                $19,
                $20,
                $21,
                $22,
                $23
            )
            RETURNING
                memory_card_id,
                $24::text,
                $25::text,
                title,
                summary,
                body,
                tags,
                provenance,
                fact_subject,
                fact_predicate,
                fact_object,
                truth_state,
                verification_state,
                status,
                derivation_kind,
                candidate_class,
                source_kind,
                hot_path_write_eligible,
                background_consolidation_recommended,
                observed_at_epoch_ms,
                recorded_at_epoch_ms,
                valid_from_epoch_ms,
                valid_to_epoch_ms,
                last_verified_at_epoch_ms,
                superseded_by_memory_card_id,
                to_char(created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"')
            "#,
            &[
                &project.project_id,
                &namespace.namespace_id,
                &title,
                &summary,
                &body,
                &string_array_json(tags),
                &stored_provenance,
                &fact_subject,
                &fact_predicate,
                &fact_object,
                &truth_state,
                &verification_state,
                &status,
                &candidate.derivation_kind,
                &candidate.candidate_class,
                &candidate.source_kind,
                &candidate.hot_path_write_eligible,
                &candidate.background_consolidation_recommended,
                &observed_at_epoch_ms,
                &recorded_at_epoch_ms,
                &valid_from_epoch_ms,
                &valid_to_epoch_ms,
                &last_verified_at_epoch_ms,
                &project.code,
                &namespace.code,
            ],
        )
        .await
        .with_context(|| {
            format!("failed to create memory card for {project_code}:{namespace_code}")
        })?;
    Ok(memory_card_record_from_row(&row))
}

pub async fn get_memory_card(client: &Client, memory_card_id: Uuid) -> Result<MemoryCardRecord> {
    let row = client
        .query_opt(
            r#"
            SELECT
                mc.memory_card_id,
                p.code,
                n.code,
                mc.title,
                mc.summary,
                mc.body,
                mc.tags,
                mc.provenance,
                mc.fact_subject,
                mc.fact_predicate,
                mc.fact_object,
                mc.truth_state,
                mc.verification_state,
                mc.status,
                mc.derivation_kind,
                mc.candidate_class,
                mc.source_kind,
                mc.hot_path_write_eligible,
                mc.background_consolidation_recommended,
                mc.observed_at_epoch_ms,
                mc.recorded_at_epoch_ms,
                mc.valid_from_epoch_ms,
                mc.valid_to_epoch_ms,
                mc.last_verified_at_epoch_ms,
                mc.superseded_by_memory_card_id,
                to_char(mc.created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"')
            FROM ami.memory_cards mc
            INNER JOIN ami.projects p ON p.project_id = mc.project_id
            INNER JOIN ami.namespaces n ON n.namespace_id = mc.namespace_id
            WHERE mc.memory_card_id = $1
            "#,
            &[&memory_card_id],
        )
        .await
        .with_context(|| format!("failed to load memory card {memory_card_id}"))?;
    let Some(row) = row else {
        return Err(anyhow!("memory card {memory_card_id} not found"));
    };
    Ok(memory_card_record_from_row(&row))
}

pub async fn get_memory_envelope(client: &Client, memory_item_id: Uuid) -> Result<Value> {
    let row = client
        .query_one(
            r#"
            SELECT row_to_json(v)::text
            FROM ami.memory_envelopes v
            WHERE v.memory_id = $1
            "#,
            &[&memory_item_id],
        )
        .await
        .with_context(|| format!("failed to get memory envelope {memory_item_id}"))?;
    let payload = row.get::<_, String>(0);
    serde_json::from_str::<Value>(&payload)
        .with_context(|| format!("failed to decode memory envelope json for {memory_item_id}"))
}

pub async fn list_memory_cards(
    client: &Client,
    project_code: Option<&str>,
    namespace_code: Option<&str>,
    truth_state: Option<&str>,
    status: Option<&str>,
) -> Result<Vec<MemoryCardRecord>> {
    let rows = client
        .query(
            r#"
            SELECT
                mc.memory_card_id,
                p.code,
                n.code,
                mc.title,
                mc.summary,
                mc.body,
                mc.tags,
                mc.provenance,
                mc.fact_subject,
                mc.fact_predicate,
                mc.fact_object,
                mc.truth_state,
                mc.verification_state,
                mc.status,
                mc.derivation_kind,
                mc.candidate_class,
                mc.source_kind,
                mc.hot_path_write_eligible,
                mc.background_consolidation_recommended,
                mc.observed_at_epoch_ms,
                mc.recorded_at_epoch_ms,
                mc.valid_from_epoch_ms,
                mc.valid_to_epoch_ms,
                mc.last_verified_at_epoch_ms,
                mc.superseded_by_memory_card_id,
                to_char(mc.created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"')
            FROM ami.memory_cards mc
            INNER JOIN ami.projects p ON p.project_id = mc.project_id
            INNER JOIN ami.namespaces n ON n.namespace_id = mc.namespace_id
            WHERE ($1::text IS NULL OR p.code = $1)
              AND ($2::text IS NULL OR n.code = $2)
              AND ($3::text IS NULL OR mc.truth_state = $3)
              AND ($4::text IS NULL OR mc.status = $4)
            ORDER BY p.code, n.code, mc.created_at DESC
            "#,
            &[&project_code, &namespace_code, &truth_state, &status],
        )
        .await
        .context("failed to list memory cards")?;
    Ok(rows.iter().map(memory_card_record_from_row).collect())
}

pub async fn supersede_memory_card(
    client: &Client,
    memory_card_id: Uuid,
    superseded_by: Uuid,
    valid_to_epoch_ms: Option<i64>,
    last_verified_at_epoch_ms: Option<i64>,
) -> Result<()> {
    let state = fetch_memory_card_state(client, memory_card_id).await?;
    let now_epoch_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock before unix epoch")?
        .as_millis() as i64;
    let valid_to_epoch_ms = valid_to_epoch_ms.or(Some(now_epoch_ms));
    let last_verified_at_epoch_ms = last_verified_at_epoch_ms.or(Some(now_epoch_ms));
    let (_, valid_to_epoch_ms) =
        normalize_temporal_bounds(state.valid_from_epoch_ms, valid_to_epoch_ms);
    client
        .execute(
            r#"
            UPDATE ami.memory_cards
            SET truth_state = 'superseded',
                status = 'superseded',
                superseded_by_memory_card_id = $2,
                valid_to_epoch_ms = COALESCE($3, valid_to_epoch_ms),
                last_verified_at_epoch_ms = COALESCE($4, last_verified_at_epoch_ms)
            WHERE memory_card_id = $1
            "#,
            &[
                &memory_card_id,
                &superseded_by,
                &valid_to_epoch_ms,
                &last_verified_at_epoch_ms,
            ],
        )
        .await
        .with_context(|| format!("failed to supersede memory card {memory_card_id}"))?;
    record_memory_card_transition(
        client,
        state.project_id,
        state.namespace_id,
        memory_card_id,
        Some(&state.truth_state),
        Some("superseded"),
        Some(&state.verification_state),
        Some(&state.verification_state),
        Some(&state.status),
        Some("superseded"),
        Some("superseded"),
        Some("memory_card_supersede"),
        Some(now_epoch_ms),
        valid_to_epoch_ms,
    )
    .await?;
    Ok(())
}

pub async fn update_memory_card_truth_state(
    client: &Client,
    memory_card_id: Uuid,
    truth_state: Option<&str>,
    verification_state: Option<&str>,
    status: Option<&str>,
    last_verified_at_epoch_ms: Option<i64>,
) -> Result<()> {
    let state = fetch_memory_card_state(client, memory_card_id).await?;
    validate_memory_card_runtime_states(
        truth_state,
        verification_state,
        status,
        "memory update-card-truth-state",
    )?;
    let effective_truth_state = truth_state.unwrap_or(state.truth_state.as_str());
    let effective_verification_state =
        verification_state.unwrap_or(state.verification_state.as_str());
    let effective_status = status.unwrap_or(state.status.as_str());
    let now_epoch_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock before unix epoch")?
        .as_millis() as i64;
    let last_verified_at_epoch_ms = last_verified_at_epoch_ms.or(Some(now_epoch_ms));
    let terminal_truth_window_close_epoch_ms =
        if matches!(effective_truth_state, "superseded" | "retracted")
            && state.valid_to_epoch_ms.is_none()
        {
            last_verified_at_epoch_ms.or(Some(now_epoch_ms))
        } else {
            None
        };
    client
        .execute(
            r#"
            UPDATE ami.memory_cards
            SET truth_state = COALESCE($2, truth_state),
                verification_state = COALESCE($3, verification_state),
                status = COALESCE($4, status),
                last_verified_at_epoch_ms = COALESCE($5, last_verified_at_epoch_ms),
                valid_to_epoch_ms = CASE
                    WHEN $6::bigint IS NOT NULL
                        THEN COALESCE(valid_to_epoch_ms, $6)
                    ELSE valid_to_epoch_ms
                END
            WHERE memory_card_id = $1
            "#,
            &[
                &memory_card_id,
                &truth_state,
                &verification_state,
                &status,
                &last_verified_at_epoch_ms,
                &terminal_truth_window_close_epoch_ms,
            ],
        )
        .await
        .with_context(|| format!("failed to update memory card truth state {memory_card_id}"))?;
    if state.truth_state != effective_truth_state
        || state.verification_state != effective_verification_state
        || state.status != effective_status
    {
        record_memory_card_transition(
            client,
            state.project_id,
            state.namespace_id,
            memory_card_id,
            Some(&state.truth_state),
            Some(effective_truth_state),
            Some(&state.verification_state),
            Some(effective_verification_state),
            Some(&state.status),
            Some(effective_status),
            Some("state_update"),
            Some("memory_card_update"),
            Some(now_epoch_ms),
            terminal_truth_window_close_epoch_ms.or(Some(now_epoch_ms)),
        )
        .await?;
    }
    Ok(())
}

pub async fn create_memory_item(
    client: &Client,
    project_code: &str,
    namespace_code: &str,
    record: &MemoryItemInsert<'_>,
) -> Result<MemoryItemRecord> {
    let (workspace_id, project, namespace) =
        resolve_scope_ids(client, project_code, namespace_code).await?;
    let workspace = get_workspace_by_id(client, workspace_id).await?;
    let source_project_id = match record.source_project_code {
        Some(code) => Some(get_project_by_code(client, code).await?.project_id),
        None => None,
    };
    let owner_agent_id = match record.owner_agent_code {
        Some(code) => find_agent_id_by_code(client, code).await?,
        None => None,
    };
    let now_epoch_ms = current_epoch_ms()?;
    let observed_at_epoch_ms = record.observed_at_epoch_ms.or(Some(now_epoch_ms));
    let recorded_at_epoch_ms = record.recorded_at_epoch_ms.or(Some(now_epoch_ms));
    let valid_from_epoch_ms = record
        .valid_from_epoch_ms
        .or(observed_at_epoch_ms)
        .or(recorded_at_epoch_ms);
    let (valid_from_epoch_ms, valid_to_epoch_ms) =
        normalize_temporal_bounds(valid_from_epoch_ms, record.valid_to_epoch_ms);
    let source_event_ids = string_array_json(record.source_event_ids);
    let artifact_refs = string_array_json(record.artifact_refs);
    let message_refs = string_array_json(record.message_refs);
    let raw_memory_event_id = Uuid::new_v4();
    let candidate = extract_memory_item_candidate(
        workspace_id,
        &project,
        &namespace,
        source_project_id,
        owner_agent_id,
        record,
        observed_at_epoch_ms,
        recorded_at_epoch_ms,
        valid_from_epoch_ms,
        valid_to_epoch_ms,
        &source_event_ids,
        &artifact_refs,
        &message_refs,
    );
    validate_memory_item_candidate(record, &candidate)?;
    let base_scope_filter = run_memory_item_policy_scope_filter(
        &workspace.code,
        &project,
        &namespace,
        owner_agent_id,
        record,
    );
    let scope_filter =
        hydrate_memory_item_policy_scope_filter(client, &project, record, base_scope_filter).await;
    validate_memory_item_policy_scope_filter(&scope_filter)?;
    let verification_check = run_memory_item_verification_conflict_check(
        client,
        &project,
        &namespace,
        record,
        &candidate,
        &scope_filter,
    )
    .await?;
    validate_memory_item_verification_conflict_check(&verification_check)?;
    let effective_visibility_scope = if let Some(import_packet_id) = record.import_packet_id {
        let packet = get_import_packet(client, import_packet_id)
            .await
            .with_context(|| format!("failed to resolve import packet {}", import_packet_id))?;
        if packet.status == "borrowed_unverified"
            || packet.verification_state != "verified"
            || packet.borrowed_status != "verified_local_copy"
        {
            "imported".to_string()
        } else {
            project.visibility_scope.clone()
        }
    } else {
        project.visibility_scope.clone()
    };
    let stored_metadata =
        augment_memory_item_metadata_with_stage2_runtime(record.metadata, &candidate);
    client
        .execute(
            r#"
            INSERT INTO ami.memory_raw_events(
                memory_raw_event_id,
                workspace_id,
                project_id,
                namespace_id,
                source_project_id,
                import_packet_id,
                owner_agent_id,
                event_kind,
                item_kind,
                visibility_scope,
                sensitivity_class,
                derivation_kind,
                truth_state,
                trust_state,
                verification_state,
                lifecycle_state,
                identity_key,
                title,
                summary,
                body,
                source_event_ids,
                artifact_refs,
                message_refs,
                evidence_span,
                causation_id,
                correlation_id,
                server_received_at_epoch_ms,
                payload
            )
            VALUES (
                $1, $2, $3, $4, $5, $6, $7,
                $8, $9, $10, COALESCE($11, 'internal'),
                COALESCE($12, 'raw_capture'),
                COALESCE($13, 'proposed'),
                COALESCE($14, 'proposed'),
                COALESCE($15, 'unverified'),
                COALESCE($16, 'hot'),
                $17, $18, $19, $20,
                $21::jsonb, $22::jsonb, $23::jsonb, $24::jsonb,
                $25, $26, $27::bigint, $28::jsonb
            )
            "#,
            &[
                &raw_memory_event_id,
                &workspace_id,
                &project.project_id,
                &namespace.namespace_id,
                &source_project_id,
                &record.import_packet_id,
                &owner_agent_id,
                &candidate.raw_event_kind,
                &record.item_kind,
                &effective_visibility_scope,
                &record.sensitivity_class,
                &record.derivation_kind,
                &record.truth_state,
                &record.trust_state,
                &record.verification_state,
                &record.lifecycle_state,
                &record.identity_key,
                &record.title,
                &record.summary,
                &record.body,
                &source_event_ids,
                &artifact_refs,
                &message_refs,
                record.evidence_span,
                &record.causation_id,
                &record.correlation_id,
                &recorded_at_epoch_ms,
                &candidate.raw_event_payload,
            ],
        )
        .await
        .with_context(|| {
            format!(
                "failed to append raw memory event for {project_code}:{namespace_code}:{}",
                raw_memory_event_id
            )
        })?;
    let row = client
        .query_one(
            r#"
            INSERT INTO ami.memory_items(
                workspace_id,
                project_id,
                namespace_id,
                source_project_id,
                import_packet_id,
                owner_agent_id,
                visibility_scope,
                item_kind,
                identity_key,
                title,
                summary,
                body,
                sensitivity_class,
                truth_state,
                trust_state,
                verification_state,
                lifecycle_state,
                source_event_ids,
                artifact_refs,
                message_refs,
                evidence_span,
                derivation_kind,
                observed_at_epoch_ms,
                recorded_at_epoch_ms,
                valid_from_epoch_ms,
                valid_to_epoch_ms,
                last_verified_at_epoch_ms,
                object_version,
                causation_id,
                correlation_id,
                utility_score,
                freshness_score,
                retention_class,
                ttl_epoch_ms,
                decay_policy,
                consolidation_status,
                imported_from,
                schema_version,
                superseded_by_memory_item_id,
                metadata
            )
            VALUES (
                $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11,
                $12, COALESCE($13, 'internal'),
                COALESCE($14, 'proposed'),
                COALESCE($15, 'proposed'),
                COALESCE($16, 'unverified'),
                COALESCE($17, 'hot'),
                $18::jsonb, $19::jsonb, $20::jsonb, $21::jsonb,
                COALESCE($22, 'raw_capture'),
                $23::bigint, $24::bigint, $25::bigint, $26::bigint, $27::bigint,
                COALESCE($28::bigint, 1), $29, $30,
                COALESCE($31::double precision, 0), COALESCE($32::double precision, 0),
                COALESCE($33, 'standard'),
                $34::bigint,
                COALESCE($35, 'standard'),
                COALESCE($36, 'active'),
                $37::jsonb, COALESCE($38, 'memory-envelope-v1'),
                $39, $40::jsonb
            )
            RETURNING
                memory_item_id,
                $41::text,
                $42::text,
                $43::text,
                $44::text,
                import_packet_id,
                owner_agent_id,
                visibility_scope,
                item_kind,
                identity_key,
                title,
                summary,
                body,
                sensitivity_class,
                truth_state,
                trust_state,
                verification_state,
                lifecycle_state,
                source_event_ids,
                artifact_refs,
                message_refs,
                evidence_span,
                derivation_kind,
                observed_at_epoch_ms,
                recorded_at_epoch_ms,
                valid_from_epoch_ms,
                valid_to_epoch_ms,
                last_verified_at_epoch_ms,
                ingest_seq,
                object_version,
                causation_id,
                correlation_id,
                utility_score,
                freshness_score,
                retention_class,
                ttl_epoch_ms,
                access_count,
                to_char(last_accessed_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),
                decay_policy,
                consolidation_status,
                imported_from,
                schema_version,
                superseded_by_memory_item_id,
                metadata
            "#,
            &[
                &workspace_id,
                &project.project_id,
                &namespace.namespace_id,
                &source_project_id,
                &record.import_packet_id,
                &owner_agent_id,
                &effective_visibility_scope,
                &record.item_kind,
                &record.identity_key,
                &record.title,
                &record.summary,
                &record.body,
                &record.sensitivity_class,
                &record.truth_state,
                &record.trust_state,
                &record.verification_state,
                &record.lifecycle_state,
                &source_event_ids,
                &artifact_refs,
                &message_refs,
                record.evidence_span,
                &record.derivation_kind,
                &observed_at_epoch_ms,
                &recorded_at_epoch_ms,
                &valid_from_epoch_ms,
                &valid_to_epoch_ms,
                &record.last_verified_at_epoch_ms,
                &record.object_version,
                &record.causation_id,
                &record.correlation_id,
                &record.utility_score,
                &record.freshness_score,
                &record.retention_class,
                &record.ttl_epoch_ms,
                &record.decay_policy,
                &record.consolidation_status,
                &candidate.imported_from,
                &record.schema_version,
                &record.superseded_by_memory_item_id,
                &stored_metadata,
                &"default",
                &project.code,
                &namespace.code,
                &record.source_project_code.map(str::to_string),
            ],
        )
        .await
        .with_context(|| {
            format!("failed to create memory item for {project_code}:{namespace_code}")
        })?;
    let memory_item = memory_item_record_from_row(&row);
    let async_index_subjects = memory_write_async_index_subjects();
    let fan_out_subjects = memory_write_fan_out_subjects();
    let write_pipeline = build_memory_write_pipeline(
        &memory_item,
        &candidate,
        &scope_filter,
        &verification_check,
        raw_memory_event_id,
        &async_index_subjects,
        &fan_out_subjects,
    );
    let basis_details = json!({
        "artifact_refs": artifact_refs,
        "message_refs": message_refs,
        "imported_from": candidate.imported_from.clone(),
        "has_evidence_span": candidate.has_evidence_span,
        "memory_raw_event_id": raw_memory_event_id,
        "write_pipeline": write_pipeline,
    });
    let provenance_details = json!({
        "artifact_refs": artifact_refs,
        "imported_from": candidate.imported_from.clone(),
        "causation_id": record.causation_id,
        "correlation_id": record.correlation_id,
        "schema_version": record.schema_version.unwrap_or("memory-envelope-v1"),
        "memory_raw_event_id": raw_memory_event_id,
        "write_pipeline": write_pipeline,
    });
    if let Some(source_kind) = candidate.source_kind.as_deref() {
        client
            .execute(
                r#"
                INSERT INTO ami.memory_provenance(
                    workspace_id,
                    project_id,
                    namespace_id,
                    memory_item_id,
                    source_kind,
                    source_event_id,
                    trust_level,
                    message_refs,
                    evidence_span,
                    derivation_kind,
                    observed_at_epoch_ms,
                    recorded_at_epoch_ms,
                    valid_from_epoch_ms,
                    valid_to_epoch_ms,
                    schema_version,
                    details
                )
                VALUES (
                    $1, $2, $3, $4, $5, $6, $7,
                    $8::jsonb, $9::jsonb, $10, $11, $12, $13, $14, $15, $16::jsonb
                )
                "#,
                &[
                    &workspace_id,
                    &project.project_id,
                    &namespace.namespace_id,
                    &memory_item.memory_item_id,
                    &source_kind,
                    &record.source_event_ids.first().cloned(),
                    &memory_item.trust_state,
                    &message_refs,
                    record.evidence_span,
                    &memory_item.derivation_kind,
                    &memory_item.observed_at_epoch_ms,
                    &memory_item.recorded_at_epoch_ms,
                    &memory_item.valid_from_epoch_ms,
                    &memory_item.valid_to_epoch_ms,
                    &memory_item.schema_version,
                    &basis_details,
                ],
            )
            .await
            .with_context(|| {
                format!(
                    "failed to record memory item source provenance for {project_code}:{namespace_code}:{}",
                    memory_item.memory_item_id
                )
            })?;
    }
    client
        .execute(
            r#"
            INSERT INTO ami.memory_provenance(
                workspace_id,
                project_id,
                namespace_id,
                memory_item_id,
                source_kind,
                source_event_id,
                trust_level,
                message_refs,
                evidence_span,
                derivation_kind,
                observed_at_epoch_ms,
                recorded_at_epoch_ms,
                valid_from_epoch_ms,
                valid_to_epoch_ms,
                schema_version,
                details
            )
            VALUES (
                $1, $2, $3, $4, 'memory_raw_event_append', $5, $6,
                $7::jsonb, $8::jsonb, $9, $10, $11, $12, $13, $14, $15::jsonb
            )
            "#,
            &[
                &workspace_id,
                &project.project_id,
                &namespace.namespace_id,
                &memory_item.memory_item_id,
                &raw_memory_event_id.to_string(),
                &memory_item.trust_state,
                &message_refs,
                record.evidence_span,
                &memory_item.derivation_kind,
                &memory_item.observed_at_epoch_ms,
                &memory_item.recorded_at_epoch_ms,
                &memory_item.valid_from_epoch_ms,
                &memory_item.valid_to_epoch_ms,
                &memory_item.schema_version,
                &json!({
                    "memory_raw_event_id": raw_memory_event_id,
                    "event_kind": candidate.raw_event_kind.clone(),
                    "payload": candidate.raw_event_payload.clone(),
                }),
            ],
        )
        .await
        .with_context(|| {
            format!(
                "failed to record raw memory event provenance for {project_code}:{namespace_code}:{}",
                memory_item.memory_item_id
            )
        })?;
    client
        .execute(
            r#"
            INSERT INTO ami.memory_provenance(
                workspace_id,
                project_id,
                namespace_id,
                memory_item_id,
                source_kind,
                source_event_id,
                trust_level,
                message_refs,
                evidence_span,
                derivation_kind,
                observed_at_epoch_ms,
                recorded_at_epoch_ms,
                valid_from_epoch_ms,
                valid_to_epoch_ms,
                schema_version,
                details
            )
            VALUES (
                $1, $2, $3, $4, 'memory_item_envelope', $5, $6,
                $7::jsonb, $8::jsonb, $9, $10, $11, $12, $13, $14, $15::jsonb
            )
            "#,
            &[
                &workspace_id,
                &project.project_id,
                &namespace.namespace_id,
                &memory_item.memory_item_id,
                &record.source_event_ids.first().cloned(),
                &memory_item.trust_state,
                &message_refs,
                record.evidence_span,
                &memory_item.derivation_kind,
                &memory_item.observed_at_epoch_ms,
                &memory_item.recorded_at_epoch_ms,
                &memory_item.valid_from_epoch_ms,
                &memory_item.valid_to_epoch_ms,
                &memory_item.schema_version,
                &provenance_details,
            ],
        )
        .await
        .with_context(|| {
            format!(
                "failed to record memory item provenance for {project_code}:{namespace_code}:{}",
                memory_item.memory_item_id
            )
        })?;
    let outbox_subjects = [
        ("ami.index.memory.lexical", "index_lexical"),
        ("ami.index.memory.graph", "index_graph"),
        ("ami.index.memory.embedding", "index_embedding"),
        ("ami.index.memory.restore_summary", "index_restore_summary"),
        ("ami.event.memory_item.created", "fanout_created"),
        (
            "ami.event.memory_item.invalidate_cache",
            "cache_invalidation",
        ),
    ];
    for (subject, delivery_kind) in outbox_subjects {
        client
            .execute(
                r#"
                INSERT INTO ami.memory_write_outbox(
                    workspace_id,
                    project_id,
                    namespace_id,
                    memory_raw_event_id,
                    memory_item_id,
                    subject,
                    delivery_kind,
                    payload
                )
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8::jsonb)
                "#,
                &[
                    &workspace_id,
                    &project.project_id,
                    &namespace.namespace_id,
                    &raw_memory_event_id,
                    &memory_item.memory_item_id,
                    &subject,
                    &delivery_kind,
                    &json!({
                        "contract_version": "memory-write-outbox-v1",
                        "workspace_id": workspace_id,
                        "project_code": project.code,
                        "namespace_code": namespace.code,
                        "memory_item_id": memory_item.memory_item_id,
                        "memory_raw_event_id": raw_memory_event_id,
                        "subject": subject,
                        "delivery_kind": delivery_kind,
                        "object_version": memory_item.object_version,
                        "ingest_seq": memory_item.ingest_seq,
                        "correlation_id": memory_item.correlation_id,
                        "causation_id": memory_item.causation_id,
                    }),
                ],
            )
            .await
            .with_context(|| {
                format!(
                    "failed to enqueue memory write outbox subject {subject} for {project_code}:{namespace_code}:{}",
                    memory_item.memory_item_id
                )
            })?;
    }
    Ok(memory_item)
}

pub async fn update_memory_item(
    client: &Client,
    record: &MemoryItemUpdate<'_>,
) -> Result<MemoryItemRecord> {
    if record.summary.is_none() && record.superseded_by_memory_item_id.is_none() {
        anyhow::bail!("memory item update requires at least one field to change");
    }
    let row = client
        .query_one(
            r#"
            WITH updated AS (
                UPDATE ami.memory_items mi
                SET
                    summary = COALESCE($2, mi.summary),
                    superseded_by_memory_item_id = COALESCE($3, mi.superseded_by_memory_item_id),
                    object_version = mi.object_version + 1
                WHERE mi.memory_item_id = $1
                RETURNING mi.*
            )
            SELECT
                u.memory_item_id,
                w.code,
                p.code,
                n.code,
                sp.code,
                u.import_packet_id,
                u.owner_agent_id,
                u.visibility_scope,
                u.item_kind,
                u.identity_key,
                u.title,
                u.summary,
                u.body,
                u.sensitivity_class,
                u.truth_state,
                u.trust_state,
                u.verification_state,
                u.lifecycle_state,
                u.source_event_ids,
                u.artifact_refs,
                u.message_refs,
                u.evidence_span,
                u.derivation_kind,
                u.observed_at_epoch_ms,
                u.recorded_at_epoch_ms,
                u.valid_from_epoch_ms,
                u.valid_to_epoch_ms,
                u.last_verified_at_epoch_ms,
                u.ingest_seq,
                u.object_version,
                u.causation_id,
                u.correlation_id,
                u.utility_score,
                u.freshness_score,
                u.retention_class,
                u.ttl_epoch_ms,
                u.imported_from,
                u.schema_version,
                u.superseded_by_memory_item_id,
                u.metadata
            FROM updated u
            INNER JOIN ami.workspaces w ON w.workspace_id = u.workspace_id
            INNER JOIN ami.projects p ON p.project_id = u.project_id
            LEFT JOIN ami.namespaces n ON n.namespace_id = u.namespace_id
            LEFT JOIN ami.projects sp ON sp.project_id = u.source_project_id
            "#,
            &[
                &record.memory_item_id,
                &record.summary,
                &record.superseded_by_memory_item_id,
            ],
        )
        .await
        .with_context(|| format!("failed to update memory item {}", record.memory_item_id))?;
    Ok(memory_item_record_from_row(&row))
}

pub async fn claim_pending_memory_write_outbox(
    client: &Client,
    limit: i64,
) -> Result<Vec<MemoryWriteOutboxDelivery>> {
    let rows = client
        .query(
            r#"
            WITH claimed AS (
                SELECT memory_write_outbox_id
                FROM ami.memory_write_outbox
                WHERE delivery_state = 'pending'
                ORDER BY created_at ASC
                LIMIT $1
                FOR UPDATE SKIP LOCKED
            )
            UPDATE ami.memory_write_outbox outbox
            SET attempt_count = outbox.attempt_count + 1
            FROM claimed
            WHERE outbox.memory_write_outbox_id = claimed.memory_write_outbox_id
            RETURNING outbox.memory_write_outbox_id, outbox.subject, outbox.payload
            "#,
            &[&limit],
        )
        .await
        .context("failed to claim pending memory write outbox rows")?;
    Ok(rows
        .into_iter()
        .map(|row| MemoryWriteOutboxDelivery {
            memory_write_outbox_id: row.get(0),
            subject: row.get(1),
            payload: row.get(2),
        })
        .collect())
}

pub async fn mark_memory_write_outbox_published(
    client: &Client,
    outbox_id: Uuid,
    published_at_epoch_ms: i64,
) -> Result<()> {
    client
        .execute(
            r#"
            UPDATE ami.memory_write_outbox
            SET delivery_state = 'published',
                published_at_epoch_ms = $2,
                last_error = NULL
            WHERE memory_write_outbox_id = $1
            "#,
            &[&outbox_id, &published_at_epoch_ms],
        )
        .await
        .with_context(|| {
            format!(
                "failed to mark memory_write_outbox {} as published",
                outbox_id
            )
        })?;
    Ok(())
}

pub async fn mark_memory_write_outbox_failed(
    client: &Client,
    outbox_id: Uuid,
    error_message: &str,
) -> Result<()> {
    client
        .execute(
            r#"
            UPDATE ami.memory_write_outbox
            SET delivery_state = 'failed',
                last_error = $2
            WHERE memory_write_outbox_id = $1
            "#,
            &[&outbox_id, &error_message],
        )
        .await
        .with_context(|| format!("failed to mark memory_write_outbox {} as failed", outbox_id))?;
    Ok(())
}

pub async fn create_task_node(
    client: &Client,
    project_code: &str,
    namespace_code: &str,
    record: &TaskNodeInsert<'_>,
) -> Result<TaskNodeRecord> {
    let (workspace_id, project, namespace) =
        resolve_scope_ids(client, project_code, namespace_code).await?;
    let source_event_ids = task_node_source_event_ids_json(record);
    let artifact_refs = task_node_artifact_refs_json(record);
    let evidence_span = task_node_evidence_span_json(record);
    let candidate =
        extract_task_node_candidate(record, &source_event_ids, &artifact_refs, &evidence_span);
    validate_task_node_candidate(&candidate)?;
    let scope_filter = run_task_node_policy_scope_filter(&project, &namespace, record);
    validate_task_node_policy_scope_filter(&scope_filter)?;
    let verification_check = run_task_node_verification_conflict_check(
        client,
        &project,
        &namespace,
        record,
        &candidate,
        &scope_filter,
    )
    .await?;
    validate_task_node_verification_conflict_check(&verification_check)?;
    let metadata = augment_task_node_metadata_with_stage2_runtime(
        record.metadata,
        &candidate,
        &scope_filter,
        &verification_check,
    );
    let row = client
        .query_one(
            r#"
            INSERT INTO ami.task_nodes(
                workspace_id,
                project_id,
                namespace_id,
                parent_task_node_id,
                memory_item_id,
                task_key,
                task_role,
                headline,
                summary,
                next_step,
                execution_state,
                lifecycle_state,
                confidence,
                current_score,
                reopened_count,
                child_count,
                closed_child_count,
                pending_return_count,
                source_event_ids,
                artifact_refs,
                evidence_span,
                candidate_class,
                derivation_kind,
                source_kind,
                hot_path_write_eligible,
                background_consolidation_recommended,
                status_payload,
                metadata,
                opened_at_epoch_ms,
                closed_at_epoch_ms,
                archived_at_epoch_ms
            )
            VALUES (
                $1, $2, $3, $4, $5, $6,
                COALESCE($7, 'workline'),
                $8, $9, $10,
                COALESCE($11, 'proposed'),
                COALESCE($12, 'hot'),
                $13, $14,
                COALESCE($15, 0),
                COALESCE($16, 0),
                COALESCE($17, 0),
                COALESCE($18, 0),
                $19::jsonb,
                $20::jsonb,
                $21::jsonb,
                $22,
                $23,
                $24,
                $25,
                $26,
                $27::jsonb,
                $28::jsonb,
                $29, $30, $31
            )
            RETURNING
                task_node_id,
                $32::text,
                $33::text,
                $34::text,
                parent_task_node_id,
                memory_item_id,
                task_key,
                task_role,
                headline,
                summary,
                next_step,
                execution_state,
                lifecycle_state,
                confidence,
                current_score,
                reopened_count,
                child_count,
                closed_child_count,
                pending_return_count,
                source_event_ids,
                artifact_refs,
                evidence_span,
                candidate_class,
                derivation_kind,
                source_kind,
                hot_path_write_eligible,
                background_consolidation_recommended,
                status_payload,
                metadata,
                opened_at_epoch_ms,
                closed_at_epoch_ms,
                archived_at_epoch_ms
            "#,
            &[
                &workspace_id,
                &project.project_id,
                &namespace.namespace_id,
                &record.parent_task_node_id,
                &record.memory_item_id,
                &record.task_key,
                &record.task_role,
                &record.headline,
                &record.summary,
                &record.next_step,
                &record.execution_state,
                &record.lifecycle_state,
                &record.confidence,
                &record.current_score,
                &record.reopened_count,
                &record.child_count,
                &record.closed_child_count,
                &record.pending_return_count,
                &source_event_ids,
                &artifact_refs,
                &evidence_span,
                &candidate.candidate_class,
                &candidate.derivation_kind,
                &candidate.source_kind,
                &candidate.hot_path_write_eligible,
                &candidate.background_consolidation_recommended,
                record.status_payload,
                &metadata,
                &record.opened_at_epoch_ms,
                &record.closed_at_epoch_ms,
                &record.archived_at_epoch_ms,
                &"default",
                &project.code,
                &namespace.code,
            ],
        )
        .await
        .with_context(|| {
            format!("failed to create task node for {project_code}:{namespace_code}")
        })?;
    let task_node = task_node_record_from_row(&row);
    if let Some(parent_task_node_id) = task_node.parent_task_node_id {
        refresh_task_node_rollups(client, parent_task_node_id).await?;
    }
    Ok(task_node)
}

pub async fn get_task_node(client: &Client, task_node_id: Uuid) -> Result<TaskNodeRecord> {
    let row = client
        .query_opt(
            r#"
            SELECT
                tn.task_node_id,
                w.code,
                p.code,
                n.code,
                tn.parent_task_node_id,
                tn.memory_item_id,
                tn.task_key,
                tn.task_role,
                tn.headline,
                tn.summary,
                tn.next_step,
                tn.execution_state,
                tn.lifecycle_state,
                tn.confidence,
                tn.current_score,
                tn.reopened_count,
                tn.child_count,
                tn.closed_child_count,
                tn.pending_return_count,
                tn.source_event_ids,
                tn.artifact_refs,
                tn.evidence_span,
                tn.candidate_class,
                tn.derivation_kind,
                tn.source_kind,
                tn.hot_path_write_eligible,
                tn.background_consolidation_recommended,
                tn.status_payload,
                tn.metadata,
                tn.opened_at_epoch_ms,
                tn.closed_at_epoch_ms,
                tn.archived_at_epoch_ms
            FROM ami.task_nodes tn
            JOIN ami.workspaces w ON w.workspace_id = tn.workspace_id
            JOIN ami.projects p ON p.project_id = tn.project_id
            LEFT JOIN ami.namespaces n ON n.namespace_id = tn.namespace_id
            WHERE tn.task_node_id = $1
            "#,
            &[&task_node_id],
        )
        .await
        .with_context(|| format!("failed to load task node {task_node_id}"))?;
    let Some(row) = row else {
        return Err(anyhow!("task node {task_node_id} not found"));
    };
    Ok(task_node_record_from_row(&row))
}

pub async fn create_task_event(
    client: &Client,
    project_code: &str,
    namespace_code: &str,
    record: &TaskEventInsert<'_>,
) -> Result<TaskEventRecord> {
    let (workspace_id, project, namespace) =
        resolve_scope_ids(client, project_code, namespace_code).await?;
    let canonical_event_kind = canonical_task_event_kind(record.event_kind);
    let recorded_at_epoch_ms = record.recorded_at_epoch_ms.or(Some(current_epoch_ms()?));
    let source_kind = record.source_kind.map(ToString::to_string).or_else(|| {
        record
            .event_payload
            .get("source_kind")
            .and_then(Value::as_str)
            .map(ToString::to_string)
    });
    let artifact_refs = task_event_json_or_empty(record.artifact_refs);
    let message_refs = task_event_json_or_empty(record.message_refs);
    let evidence_span = task_event_evidence_span_json(record);
    validate_task_event_basis(record, &artifact_refs, &message_refs, &evidence_span)?;
    let derivation_kind = record.derivation_kind.unwrap_or("raw_capture").to_string();
    let schema_version = record
        .schema_version
        .unwrap_or("task-event-envelope-v1")
        .to_string();
    let row = client
        .query_one(
            r#"
            INSERT INTO ami.task_events(
                workspace_id,
                project_id,
                namespace_id,
                task_node_id,
                source_snapshot_id,
                source_event_id,
                event_kind,
                prior_execution_state,
                next_execution_state,
                prior_lifecycle_state,
                next_lifecycle_state,
                source_kind,
                artifact_refs,
                message_refs,
                evidence_span,
                derivation_kind,
                schema_version,
                event_payload,
                recorded_at_epoch_ms
            )
            VALUES (
                $1, $2, $3, $4, $5, $6, $7,
                $8, $9, $10, $11,
                $12, $13::jsonb, $14::jsonb, $15::jsonb, $16, $17,
                $18::jsonb, $19
            )
            RETURNING
                task_event_id,
                $20::text,
                $21::text,
                $22::text,
                task_node_id,
                source_snapshot_id,
                source_event_id,
                event_kind,
                prior_execution_state,
                next_execution_state,
                prior_lifecycle_state,
                next_lifecycle_state,
                source_kind,
                artifact_refs,
                message_refs,
                evidence_span,
                derivation_kind,
                schema_version,
                event_payload,
                recorded_at_epoch_ms
            "#,
            &[
                &workspace_id,
                &project.project_id,
                &namespace.namespace_id,
                &record.task_node_id,
                &record.source_snapshot_id,
                &record.source_event_id,
                &canonical_event_kind,
                &record.prior_execution_state,
                &record.next_execution_state,
                &record.prior_lifecycle_state,
                &record.next_lifecycle_state,
                &source_kind,
                &artifact_refs,
                &message_refs,
                &evidence_span,
                &derivation_kind,
                &schema_version,
                record.event_payload,
                &recorded_at_epoch_ms,
                &"default",
                &project.code,
                &namespace.code,
            ],
        )
        .await
        .with_context(|| {
            format!("failed to create task event for {project_code}:{namespace_code}")
        })?;
    apply_task_event_to_task_node(
        client,
        record.task_node_id,
        canonical_event_kind,
        record.next_execution_state,
        record.prior_lifecycle_state,
        record.next_lifecycle_state,
        recorded_at_epoch_ms.expect("recorded_at_epoch_ms is materialized"),
    )
    .await?;
    Ok(task_event_record_from_row(&row))
}

pub async fn get_task_event(client: &Client, task_event_id: Uuid) -> Result<TaskEventRecord> {
    let row = client
        .query_opt(
            r#"
            SELECT
                te.task_event_id,
                w.code,
                p.code,
                n.code,
                te.task_node_id,
                te.source_snapshot_id,
                te.source_event_id,
                te.event_kind,
                te.prior_execution_state,
                te.next_execution_state,
                te.prior_lifecycle_state,
                te.next_lifecycle_state,
                te.source_kind,
                te.artifact_refs,
                te.message_refs,
                te.evidence_span,
                te.derivation_kind,
                te.schema_version,
                te.event_payload,
                te.recorded_at_epoch_ms
            FROM ami.task_events te
            JOIN ami.workspaces w ON w.workspace_id = te.workspace_id
            JOIN ami.projects p ON p.project_id = te.project_id
            LEFT JOIN ami.namespaces n ON n.namespace_id = te.namespace_id
            WHERE te.task_event_id = $1
            "#,
            &[&task_event_id],
        )
        .await
        .with_context(|| format!("failed to load task event {task_event_id}"))?;
    let Some(row) = row else {
        return Err(anyhow!("task event {task_event_id} not found"));
    };
    Ok(task_event_record_from_row(&row))
}

pub async fn create_memory_link_decision(
    client: &Client,
    project_code: &str,
    namespace_code: &str,
    record: &MemoryLinkDecisionInsert<'_>,
) -> Result<MemoryLinkDecisionRecord> {
    let (workspace_id, project, namespace) =
        resolve_scope_ids(client, project_code, namespace_code).await?;
    let recorded_at_epoch_ms = record.recorded_at_epoch_ms.or(Some(current_epoch_ms()?));
    let source_event_ids = link_surface_json_or_empty(record.source_event_ids);
    let artifact_refs = link_surface_json_or_empty(record.artifact_refs);
    let message_refs = link_surface_json_or_empty(record.message_refs);
    let evidence_span =
        link_surface_evidence_span_json(record.evidence_span, record.decision_payload);
    validate_memory_link_decision_basis(
        record,
        &source_event_ids,
        &artifact_refs,
        &message_refs,
        &evidence_span,
    )?;
    let derivation_kind = record.derivation_kind.unwrap_or("extract").to_string();
    let schema_version = record
        .schema_version
        .unwrap_or("memory-link-decision-envelope-v1")
        .to_string();
    let policy_filter = run_memory_link_decision_policy_scope_filter(
        client,
        &project,
        &namespace,
        record.task_node_id,
        record.candidate_task_node_id,
        record.retrieval_trace_id,
    )
    .await;
    validate_memory_link_decision_policy_scope_filter(&policy_filter)?;
    let verification_check = run_memory_link_decision_verification_conflict_check(
        &derivation_kind,
        &source_event_ids,
        &artifact_refs,
        &message_refs,
        &evidence_span,
        &policy_filter,
        record.retrieval_trace_id.is_some(),
    );
    validate_memory_link_decision_verification_conflict_check(&verification_check)?;
    let stored_evidence_span = augment_memory_link_decision_evidence_span_with_stage2_preflight(
        &evidence_span,
        &policy_filter,
        &verification_check,
    );
    let row = client
        .query_one(
            r#"
            INSERT INTO ami.memory_link_decisions(
                workspace_id,
                project_id,
                namespace_id,
                task_node_id,
                retrieval_trace_id,
                candidate_task_node_id,
                decision_outcome,
                legality_passed,
                scope_filter_passed,
                evidence_sufficient,
                classifier_label,
                classifier_score,
                decision_reason,
                decision_payload,
                source_event_ids,
                artifact_refs,
                message_refs,
                evidence_span,
                derivation_kind,
                schema_version,
                recorded_at_epoch_ms
            )
            VALUES (
                $1, $2, $3, $4, $5, $6, $7,
                $8, $9, $10, $11, $12, $13, $14::jsonb,
                $15::jsonb, $16::jsonb, $17::jsonb, $18::jsonb, $19, $20, $21
            )
            RETURNING
                memory_link_decision_id,
                $22::text,
                $23::text,
                $24::text,
                task_node_id,
                retrieval_trace_id,
                candidate_task_node_id,
                decision_outcome,
                legality_passed,
                scope_filter_passed,
                evidence_sufficient,
                classifier_label,
                classifier_score,
                decision_reason,
                decision_payload,
                source_event_ids,
                artifact_refs,
                message_refs,
                evidence_span,
                derivation_kind,
                schema_version,
                recorded_at_epoch_ms
            "#,
            &[
                &workspace_id,
                &project.project_id,
                &namespace.namespace_id,
                &record.task_node_id,
                &record.retrieval_trace_id,
                &record.candidate_task_node_id,
                &record.decision_outcome,
                &record.legality_passed,
                &record.scope_filter_passed,
                &record.evidence_sufficient,
                &record.classifier_label,
                &record.classifier_score,
                &record.decision_reason,
                record.decision_payload,
                &source_event_ids,
                &artifact_refs,
                &message_refs,
                &stored_evidence_span,
                &derivation_kind,
                &schema_version,
                &recorded_at_epoch_ms,
                &"default",
                &project.code,
                &namespace.code,
            ],
        )
        .await
        .with_context(|| {
            format!("failed to create memory link decision for {project_code}:{namespace_code}")
        })?;
    let decision = memory_link_decision_record_from_row(&row);
    let should_materialize = decision.legality_passed
        && decision.scope_filter_passed
        && (decision.evidence_sufficient
            || matches!(
                decision.decision_outcome.as_str(),
                "abstain" | "escalate" | "pending_link_proposal"
            ));
    if should_materialize {
        materialize_memory_link_decision_onto_task_graph(
            client,
            project_code,
            namespace_code,
            &decision,
        )
        .await?;
    }
    Ok(decision)
}

pub async fn get_memory_link_decision(
    client: &Client,
    memory_link_decision_id: Uuid,
) -> Result<MemoryLinkDecisionRecord> {
    let row = client
        .query_opt(
            r#"
            SELECT
                mld.memory_link_decision_id,
                w.code,
                p.code,
                n.code,
                mld.task_node_id,
                mld.retrieval_trace_id,
                mld.candidate_task_node_id,
                mld.decision_outcome,
                mld.legality_passed,
                mld.scope_filter_passed,
                mld.evidence_sufficient,
                mld.classifier_label,
                mld.classifier_score,
                mld.decision_reason,
                mld.decision_payload,
                mld.source_event_ids,
                mld.artifact_refs,
                mld.message_refs,
                mld.evidence_span,
                mld.derivation_kind,
                mld.schema_version,
                mld.recorded_at_epoch_ms
            FROM ami.memory_link_decisions mld
            JOIN ami.workspaces w ON w.workspace_id = mld.workspace_id
            JOIN ami.projects p ON p.project_id = mld.project_id
            LEFT JOIN ami.namespaces n ON n.namespace_id = mld.namespace_id
            WHERE mld.memory_link_decision_id = $1
            "#,
            &[&memory_link_decision_id],
        )
        .await
        .with_context(|| {
            format!("failed to load memory link decision {memory_link_decision_id}")
        })?;
    let Some(row) = row else {
        return Err(anyhow!(
            "memory link decision {memory_link_decision_id} not found"
        ));
    };
    Ok(memory_link_decision_record_from_row(&row))
}

pub async fn create_pending_link_proposal(
    client: &Client,
    project_code: &str,
    namespace_code: &str,
    record: &PendingLinkProposalInsert<'_>,
) -> Result<PendingLinkProposalRecord> {
    let (workspace_id, project, namespace) =
        resolve_scope_ids(client, project_code, namespace_code).await?;
    let source_event_ids = link_surface_json_or_empty(record.source_event_ids);
    let artifact_refs = link_surface_json_or_empty(record.artifact_refs);
    let message_refs = link_surface_json_or_empty(record.message_refs);
    let evidence_span =
        link_surface_evidence_span_json(record.evidence_span, record.evidence_payload);
    validate_pending_link_proposal_basis(
        record,
        &source_event_ids,
        &artifact_refs,
        &message_refs,
        &evidence_span,
    )?;
    let derivation_kind = record.derivation_kind.unwrap_or("extract").to_string();
    let schema_version = record
        .schema_version
        .unwrap_or("pending-link-proposal-envelope-v1")
        .to_string();
    let policy_filter = run_memory_link_decision_policy_scope_filter(
        client,
        &project,
        &namespace,
        record.task_node_id,
        record.candidate_task_node_id,
        record.retrieval_trace_id,
    )
    .await;
    validate_memory_link_decision_policy_scope_filter(&policy_filter)?;
    let verification_check = run_memory_link_decision_verification_conflict_check(
        &derivation_kind,
        &source_event_ids,
        &artifact_refs,
        &message_refs,
        &evidence_span,
        &policy_filter,
        record.retrieval_trace_id.is_some(),
    );
    validate_memory_link_decision_verification_conflict_check(&verification_check)?;
    let stored_evidence_span = augment_memory_link_decision_evidence_span_with_stage2_preflight(
        &evidence_span,
        &policy_filter,
        &verification_check,
    );
    let row = client
        .query_one(
            r#"
            INSERT INTO ami.pending_link_proposals(
                workspace_id,
                project_id,
                namespace_id,
                task_node_id,
                retrieval_trace_id,
                candidate_task_node_id,
                proposal_state,
                proposal_reason,
                evidence_request,
                evidence_payload,
                classifier_score,
                ttl_epoch_ms,
                source_event_ids,
                artifact_refs,
                message_refs,
                evidence_span,
                derivation_kind,
                schema_version
            )
            VALUES (
                $1, $2, $3, $4, $5, $6,
                COALESCE($7, 'pending'),
                $8, $9, $10::jsonb, $11, $12,
                $13::jsonb, $14::jsonb, $15::jsonb, $16::jsonb, $17, $18
            )
            RETURNING
                pending_link_proposal_id,
                $19::text,
                $20::text,
                $21::text,
                task_node_id,
                retrieval_trace_id,
                candidate_task_node_id,
                proposal_state,
                proposal_reason,
                evidence_request,
                evidence_payload,
                classifier_score,
                ttl_epoch_ms,
                source_event_ids,
                artifact_refs,
                message_refs,
                evidence_span,
                derivation_kind,
                schema_version
            "#,
            &[
                &workspace_id,
                &project.project_id,
                &namespace.namespace_id,
                &record.task_node_id,
                &record.retrieval_trace_id,
                &record.candidate_task_node_id,
                &record.proposal_state,
                &record.proposal_reason,
                &record.evidence_request,
                record.evidence_payload,
                &record.classifier_score,
                &record.ttl_epoch_ms,
                &source_event_ids,
                &artifact_refs,
                &message_refs,
                &stored_evidence_span,
                &derivation_kind,
                &schema_version,
                &"default",
                &project.code,
                &namespace.code,
            ],
        )
        .await
        .with_context(|| {
            format!("failed to create pending link proposal for {project_code}:{namespace_code}")
        })?;
    let proposal = pending_link_proposal_record_from_row(&row);
    materialize_pending_link_proposal_onto_task_graph(
        client,
        project_code,
        namespace_code,
        &proposal,
    )
    .await?;
    Ok(proposal)
}

pub async fn get_pending_link_proposal(
    client: &Client,
    pending_link_proposal_id: Uuid,
) -> Result<PendingLinkProposalRecord> {
    let row = client
        .query_opt(
            r#"
            SELECT
                plp.pending_link_proposal_id,
                w.code,
                p.code,
                n.code,
                plp.task_node_id,
                plp.retrieval_trace_id,
                plp.candidate_task_node_id,
                plp.proposal_state,
                plp.proposal_reason,
                plp.evidence_request,
                plp.evidence_payload,
                plp.classifier_score,
                plp.ttl_epoch_ms,
                plp.source_event_ids,
                plp.artifact_refs,
                plp.message_refs,
                plp.evidence_span,
                plp.derivation_kind,
                plp.schema_version
            FROM ami.pending_link_proposals plp
            JOIN ami.workspaces w ON w.workspace_id = plp.workspace_id
            JOIN ami.projects p ON p.project_id = plp.project_id
            LEFT JOIN ami.namespaces n ON n.namespace_id = plp.namespace_id
            WHERE plp.pending_link_proposal_id = $1
            "#,
            &[&pending_link_proposal_id],
        )
        .await
        .with_context(|| {
            format!("failed to load pending link proposal {pending_link_proposal_id}")
        })?;
    let Some(row) = row else {
        return Err(anyhow!(
            "pending link proposal {pending_link_proposal_id} not found"
        ));
    };
    Ok(pending_link_proposal_record_from_row(&row))
}

pub async fn find_task_node_by_task_key(
    client: &Client,
    project_id: Uuid,
    namespace_id: Uuid,
    task_key: &str,
) -> Result<Option<TaskNodeRecord>> {
    let row = client
        .query_opt(
            r#"
            SELECT
                tn.task_node_id,
                w.code,
                p.code,
                n.code,
                tn.parent_task_node_id,
                tn.memory_item_id,
                tn.task_key,
                tn.task_role,
                tn.headline,
                tn.summary,
                tn.next_step,
                tn.execution_state,
                tn.lifecycle_state,
                tn.confidence,
                tn.current_score,
                tn.reopened_count,
                tn.child_count,
                tn.closed_child_count,
                tn.pending_return_count,
                tn.source_event_ids,
                tn.artifact_refs,
                tn.evidence_span,
                tn.candidate_class,
                tn.derivation_kind,
                tn.source_kind,
                tn.hot_path_write_eligible,
                tn.background_consolidation_recommended,
                tn.status_payload,
                tn.metadata,
                tn.opened_at_epoch_ms,
                tn.closed_at_epoch_ms,
                tn.archived_at_epoch_ms
            FROM ami.task_nodes tn
            JOIN ami.workspaces w ON w.workspace_id = tn.workspace_id
            JOIN ami.projects p ON p.project_id = tn.project_id
            LEFT JOIN ami.namespaces n ON n.namespace_id = tn.namespace_id
            WHERE tn.project_id = $1
              AND tn.namespace_id = $2
              AND tn.task_key = $3
            ORDER BY tn.updated_at DESC, tn.created_at DESC
            LIMIT 1
            "#,
            &[&project_id, &namespace_id, &task_key],
        )
        .await
        .context("failed to find task node by task_key")?;
    Ok(row.as_ref().map(task_node_record_from_row))
}

pub async fn list_memory_relation_edges_for_cards(
    client: &Client,
    project_id: Uuid,
    namespace_id: Uuid,
    memory_card_ids: &[Uuid],
    at_epoch_ms: Option<i64>,
    limit: i64,
) -> Result<Vec<MemoryRelationEdgeRecord>> {
    if memory_card_ids.is_empty() || limit <= 0 {
        return Ok(Vec::new());
    }
    let rows = client
        .query(
            r#"
            SELECT
                mre.memory_relation_edge_id,
                p.code,
                n.code,
                mre.source_memory_card_id,
                mre.target_memory_card_id,
                mre.relation_type,
                mre.relation_state,
                mre.evidence,
                mre.source_kind,
                mre.source_event_ids,
                mre.artifact_refs,
                mre.message_refs,
                mre.evidence_span,
                mre.derivation_kind,
                mre.schema_version,
                mre.recorded_at_epoch_ms,
                mre.valid_from_epoch_ms,
                mre.valid_to_epoch_ms,
                to_char(mre.created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"')
            FROM ami.memory_relation_edges mre
            JOIN ami.projects p ON p.project_id = mre.project_id
            JOIN ami.namespaces n ON n.namespace_id = mre.namespace_id
            WHERE mre.project_id = $1
              AND mre.namespace_id = $2
              AND (
                    mre.source_memory_card_id = ANY($3)
                 OR mre.target_memory_card_id = ANY($3)
              )
              AND (
                    $4::bigint IS NULL
                 OR (
                        (mre.valid_from_epoch_ms IS NULL OR mre.valid_from_epoch_ms <= $4)
                    AND (mre.valid_to_epoch_ms IS NULL OR mre.valid_to_epoch_ms >= $4)
                 )
              )
            ORDER BY mre.created_at DESC
            LIMIT $5
            "#,
            &[
                &project_id,
                &namespace_id,
                &memory_card_ids,
                &at_epoch_ms,
                &limit,
            ],
        )
        .await?;
    Ok(rows
        .iter()
        .map(memory_relation_edge_record_from_row)
        .collect())
}

pub async fn apply_memory_card_update(
    client: &Client,
    project_code: &str,
    namespace_code: &str,
    title: &str,
    summary: &str,
    body: &str,
    tags: &[String],
    provenance: &Value,
    fact_subject: Option<&str>,
    fact_predicate: Option<&str>,
    fact_object: Option<&str>,
    truth_state: Option<&str>,
    verification_state: Option<&str>,
    status: Option<&str>,
    observed_at_epoch_ms: Option<i64>,
    recorded_at_epoch_ms: Option<i64>,
    valid_from_epoch_ms: Option<i64>,
    valid_to_epoch_ms: Option<i64>,
    last_verified_at_epoch_ms: Option<i64>,
) -> Result<MemoryCardRecord> {
    validate_memory_card_runtime_states(
        truth_state,
        verification_state,
        status,
        "memory apply-card-update",
    )?;
    let new_card = create_memory_card(
        client,
        project_code,
        namespace_code,
        title,
        summary,
        body,
        tags,
        provenance,
        fact_subject,
        fact_predicate,
        fact_object,
        truth_state,
        verification_state,
        status,
        observed_at_epoch_ms,
        recorded_at_epoch_ms,
        valid_from_epoch_ms,
        valid_to_epoch_ms,
        last_verified_at_epoch_ms,
    )
    .await?;

    if fact_subject.is_some() && fact_predicate.is_some() {
        let project = get_project_by_code(client, project_code).await?;
        let namespace = get_namespace_by_code(client, project.project_id, namespace_code).await?;
        let rows = client
            .query(
                r#"
                SELECT memory_card_id, fact_object
                FROM ami.memory_cards
                WHERE project_id = $1
                  AND namespace_id = $2
                  AND fact_subject = $3
                  AND fact_predicate = $4
                  AND truth_state = 'current'
                  AND status = 'active'
                  AND memory_card_id <> $5
                ORDER BY created_at DESC
                "#,
                &[
                    &project.project_id,
                    &namespace.namespace_id,
                    &fact_subject,
                    &fact_predicate,
                    &new_card.memory_card_id,
                ],
            )
            .await?;
        for row in rows {
            let old_card_id: Uuid = row.get(0);
            let old_fact_object: Option<String> = row.get(1);
            let supersession_reason = match (old_fact_object.as_deref(), fact_object) {
                (Some(old_object), Some(new_object)) if old_object != new_object => {
                    "knowledge_update_object_change"
                }
                _ => "knowledge_update_refresh",
            };
            supersede_memory_card(
                client,
                old_card_id,
                new_card.memory_card_id,
                observed_at_epoch_ms.or(recorded_at_epoch_ms),
                last_verified_at_epoch_ms,
            )
            .await?;
            let _ = create_memory_relation_edge(
                client,
                project_code,
                namespace_code,
                new_card.memory_card_id,
                old_card_id,
                "supersedes",
                Some("active"),
                &json!({
                    "reason": "knowledge_update",
                    "supersession_reason": supersession_reason,
                    "fact_subject": fact_subject,
                    "fact_predicate": fact_predicate,
                    "new_fact_object": fact_object,
                    "old_fact_object": old_fact_object,
                }),
                Some("memory_card_update"),
                None,
                Some(&json!([format!("memory-card:{}", new_card.memory_card_id)])),
                None,
                Some(&json!({
                    "reason": "knowledge_update",
                    "supersession_reason": supersession_reason,
                    "new_memory_card_id": new_card.memory_card_id,
                    "old_memory_card_id": old_card_id,
                    "fact_subject": fact_subject,
                    "fact_predicate": fact_predicate,
                    "new_fact_object": fact_object,
                    "old_fact_object": old_fact_object,
                })),
                Some("merge"),
                Some("memory-relation-edge-envelope-v1"),
                observed_at_epoch_ms.or(recorded_at_epoch_ms),
                observed_at_epoch_ms.or(recorded_at_epoch_ms),
                None,
            )
            .await?;
        }
    }

    Ok(new_card)
}
