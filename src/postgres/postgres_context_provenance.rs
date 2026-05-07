use super::*;
use anyhow::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RestorePackCreateErrorPhase {
    BeforeWrite,
    OutcomeUnknownAfterWrite,
}

#[derive(Debug)]
pub(crate) struct RestorePackCreateError {
    pub(crate) phase: RestorePackCreateErrorPhase,
    pub(crate) project_code: String,
    pub(crate) namespace_code: String,
    pub(crate) pack_kind: String,
    pub(crate) source_snapshot_id: Option<Uuid>,
    pub(crate) error: Error,
}

impl RestorePackCreateError {
    fn before_write(
        project_code: &str,
        namespace_code: &str,
        pack_kind: &str,
        source_snapshot_id: Option<Uuid>,
        error: Error,
    ) -> Self {
        Self {
            phase: RestorePackCreateErrorPhase::BeforeWrite,
            project_code: project_code.to_string(),
            namespace_code: namespace_code.to_string(),
            pack_kind: pack_kind.to_string(),
            source_snapshot_id,
            error,
        }
    }

    fn outcome_unknown_after_write(
        project_code: &str,
        namespace_code: &str,
        pack_kind: &str,
        source_snapshot_id: Option<Uuid>,
        error: Error,
    ) -> Self {
        Self {
            phase: RestorePackCreateErrorPhase::OutcomeUnknownAfterWrite,
            project_code: project_code.to_string(),
            namespace_code: namespace_code.to_string(),
            pack_kind: pack_kind.to_string(),
            source_snapshot_id,
            error,
        }
    }
}

fn restore_pack_source_snapshot_advisory_lock_key(
    namespace_id: Uuid,
    pack_kind: &str,
    source_snapshot_id: Uuid,
) -> i64 {
    let mut hasher = Sha256::new();
    hasher.update(namespace_id.as_bytes());
    hasher.update(pack_kind.as_bytes());
    hasher.update(source_snapshot_id.as_bytes());
    let digest = hasher.finalize();
    let mut bytes = [0_u8; 8];
    bytes.copy_from_slice(&digest[..8]);
    i64::from_be_bytes(bytes)
}

fn ensure_existing_restore_pack_matches_incoming(
    existing: &RestorePackRecord,
    record: &RestorePackInsert<'_>,
    source_event_ids: &Value,
    artifact_refs: &Value,
    message_refs: &Value,
    stored_evidence_span: &Value,
    derivation_kind: &str,
    schema_version: &str,
) -> Result<()> {
    let incoming_source_kind = record.source_kind.map(ToOwned::to_owned);
    let incoming_headline = record.headline.map(ToOwned::to_owned);
    let incoming_summary = record.summary.map(ToOwned::to_owned);

    let same_canonical_content = existing.source_kind == incoming_source_kind
        && existing.source_event_ids == *source_event_ids
        && existing.artifact_refs == *artifact_refs
        && existing.message_refs == *message_refs
        && existing.evidence_span == *stored_evidence_span
        && existing.derivation_kind == derivation_kind
        && existing.schema_version == schema_version
        && existing.headline == incoming_headline
        && existing.summary == incoming_summary
        && existing.payload == *record.payload
        && existing.captured_at_epoch_ms == record.captured_at_epoch_ms;

    if same_canonical_content {
        return Ok(());
    }

    Err(anyhow!(
        "restore pack canonical content conflict for existing restore_pack_id={} project={} namespace={} pack_kind={} source_snapshot_id={:?}",
        existing.restore_pack_id,
        existing.project_code,
        existing.namespace_code.as_deref().unwrap_or_default(),
        existing.pack_kind,
        existing.source_snapshot_id
    ))
}

fn validate_restore_pack_record_source_identity(record: &RestorePackRecord) -> Result<()> {
    if record.pack_kind == "workspace_restore_pack" && record.source_snapshot_id.is_none() {
        return Err(anyhow!(
            "restore pack {} violates source identity law: workspace_restore_pack requires source_snapshot_id",
            record.restore_pack_id
        ));
    }
    Ok(())
}

#[cfg(test)]
async fn maybe_delay_restore_pack_create_after_lookup_for_tests() {
    let delay_ms = std::env::var("AMAI_TEST_DELAY_RESTORE_PACK_CREATE_AFTER_LOOKUP_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);
    if delay_ms > 0 {
        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
    }
}

#[cfg(test)]
fn forced_restore_pack_create_error_for_tests(
    project_code: &str,
    namespace_code: &str,
    pack_kind: &str,
    source_snapshot_id: Option<Uuid>,
    after_write: bool,
) -> Option<RestorePackCreateError> {
    let spec = std::env::var("AMAI_TEST_FORCE_RESTORE_PACK_CREATE_FAILURE").ok()?;
    let trimmed = spec.trim();
    if trimmed.is_empty() {
        return None;
    }
    match (trimmed, after_write) {
        ("before_write", false) => Some(RestorePackCreateError::before_write(
            project_code,
            namespace_code,
            pack_kind,
            source_snapshot_id,
            anyhow!(
                "forced restore pack create failure for tests phase=before_write project={} namespace={} pack_kind={}",
                project_code,
                namespace_code,
                pack_kind
            ),
        )),
        ("outcome_unknown_after_write", true) => Some(
            RestorePackCreateError::outcome_unknown_after_write(
                project_code,
                namespace_code,
                pack_kind,
                source_snapshot_id,
                anyhow!(
                    "forced restore pack create failure for tests phase=outcome_unknown_after_write project={} namespace={} pack_kind={}",
                    project_code,
                    namespace_code,
                    pack_kind
                ),
            ),
        ),
        _ => None,
    }
}

async fn create_restore_pack_row(
    client: &Client,
    workspace_id: Uuid,
    workspace_code: &str,
    project: &ProjectRecord,
    namespace: &NamespaceRecord,
    record: &RestorePackInsert<'_>,
    source_event_ids: &Value,
    artifact_refs: &Value,
    message_refs: &Value,
    stored_evidence_span: &Value,
    derivation_kind: &str,
    schema_version: &str,
) -> Result<RestorePackRecord> {
    let row = client
        .query_one(
            r#"
            INSERT INTO ami.restore_packs(
                workspace_id,
                project_id,
                namespace_id,
                agent_scope,
                session_id,
                thread_id,
                source_snapshot_id,
                pack_kind,
                source_kind,
                source_event_ids,
                artifact_refs,
                message_refs,
                evidence_span,
                derivation_kind,
                schema_version,
                headline,
                summary,
                payload,
                captured_at_epoch_ms
            )
            VALUES (
                $1, $2, $3, $4, $5, $6, $7, $8,
                $9, $10::jsonb, $11::jsonb, $12::jsonb, $13::jsonb,
                $14, $15, $16, $17, $18::jsonb, $19
            )
            RETURNING
                restore_pack_id,
                $20::text,
                $21::text,
                $22::text,
                agent_scope,
                session_id,
                thread_id,
                source_snapshot_id,
                pack_kind,
                source_kind,
                source_event_ids,
                artifact_refs,
                message_refs,
                evidence_span,
                derivation_kind,
                schema_version,
                headline,
                summary,
                payload,
                captured_at_epoch_ms
            "#,
            &[
                &workspace_id,
                &project.project_id,
                &namespace.namespace_id,
                &record.agent_scope,
                &record.session_id,
                &record.thread_id,
                &record.source_snapshot_id,
                &record.pack_kind,
                &record.source_kind,
                source_event_ids,
                artifact_refs,
                message_refs,
                stored_evidence_span,
                &derivation_kind,
                &schema_version,
                &record.headline,
                &record.summary,
                record.payload,
                &record.captured_at_epoch_ms,
                &workspace_code,
                &project.code,
                &namespace.code,
            ],
        )
        .await
        .with_context(|| {
            format!(
                "failed to create restore pack for {}:{}",
                project.code, namespace.code
            )
        })?;
    let record = restore_pack_record_from_row(&row);
    validate_restore_pack_record_source_identity(&record)?;
    Ok(record)
}

pub(super) fn validate_artifact_ref_basis(
    record: &ArtifactRefInsert<'_>,
    source_event_ids: &Value,
    message_refs: &Value,
    evidence_span: &Value,
) -> Result<()> {
    let derivation_kind = record.derivation_kind.unwrap_or("extract");
    let has_source_events = value_string_array_len(Some(source_event_ids)) > 0;
    let has_message_refs = value_string_array_len(Some(message_refs)) > 0;
    let has_evidence_span = evidence_span
        .as_object()
        .is_some_and(|span| !span.is_empty() && span.values().any(|value| !value.is_null()));
    if derivation_kind != "operator_write"
        && !has_source_events
        && !has_message_refs
        && !has_evidence_span
    {
        return Err(anyhow!(
            "artifact ref requires recorded basis unless derivation_kind=operator_write"
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
struct ArtifactRefPolicyScopeFilter {
    workspace_code: String,
    project_code: String,
    namespace_code: String,
    requested_project_matches: bool,
    requested_namespace_matches: bool,
    scope_binding_valid: bool,
}

#[derive(Debug, Clone, Serialize)]
struct ArtifactRefVerificationConflictCheck {
    evidence_present: bool,
    poisoned_detected: bool,
    write_allowed: bool,
}

fn artifact_ref_marks_poisoned(evidence_span: &Value) -> bool {
    evidence_span
        .get("poisoned")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || evidence_span
            .get("safety")
            .and_then(Value::as_object)
            .and_then(|safety| safety.get("poisoned"))
            .and_then(Value::as_bool)
            .unwrap_or(false)
}

fn run_artifact_ref_policy_scope_filter(
    workspace_code: &str,
    project: &ProjectRecord,
    namespace: &NamespaceRecord,
    record: &ArtifactRefInsert<'_>,
) -> ArtifactRefPolicyScopeFilter {
    let requested_project_matches = record.project_id == project.project_id;
    let requested_namespace_matches = record.namespace_id == namespace.namespace_id;
    ArtifactRefPolicyScopeFilter {
        workspace_code: workspace_code.to_string(),
        project_code: project.code.clone(),
        namespace_code: namespace.code.clone(),
        requested_project_matches,
        requested_namespace_matches,
        scope_binding_valid: requested_project_matches && requested_namespace_matches,
    }
}

fn validate_artifact_ref_policy_scope_filter(filter: &ArtifactRefPolicyScopeFilter) -> Result<()> {
    if !filter.requested_project_matches {
        return Err(anyhow!(
            "artifact ref project binding does not match target {}:{}",
            filter.project_code,
            filter.namespace_code
        ));
    }
    if !filter.requested_namespace_matches {
        return Err(anyhow!(
            "artifact ref namespace binding does not match target {}:{}",
            filter.project_code,
            filter.namespace_code
        ));
    }
    Ok(())
}

fn run_artifact_ref_verification_conflict_check(
    derivation_kind: &str,
    source_event_ids: &Value,
    message_refs: &Value,
    evidence_span: &Value,
    policy_filter: &ArtifactRefPolicyScopeFilter,
) -> ArtifactRefVerificationConflictCheck {
    let evidence_present = derivation_kind == "operator_write"
        || value_string_array_len(Some(source_event_ids)) > 0
        || value_string_array_len(Some(message_refs)) > 0
        || evidence_span
            .as_object()
            .is_some_and(|span| !span.is_empty() && span.values().any(|value| !value.is_null()));
    let poisoned_detected = artifact_ref_marks_poisoned(evidence_span);
    ArtifactRefVerificationConflictCheck {
        evidence_present,
        poisoned_detected,
        write_allowed: policy_filter.scope_binding_valid && evidence_present && !poisoned_detected,
    }
}

fn validate_artifact_ref_verification_conflict_check(
    check: &ArtifactRefVerificationConflictCheck,
) -> Result<()> {
    if check.poisoned_detected {
        return Err(anyhow!("artifact ref evidence_span is flagged poisoned"));
    }
    if !check.evidence_present {
        return Err(anyhow!("artifact ref requires recorded evidence"));
    }
    if !check.write_allowed {
        return Err(anyhow!(
            "artifact ref verification/conflict check blocked write"
        ));
    }
    Ok(())
}

fn augment_artifact_ref_evidence_span_with_stage2_preflight(
    evidence_span: &Value,
    policy_filter: &ArtifactRefPolicyScopeFilter,
    verification_check: &ArtifactRefVerificationConflictCheck,
) -> Value {
    let mut enriched = match evidence_span {
        Value::Object(map) => map.clone(),
        _ => serde_json::Map::new(),
    };
    enriched.insert(
        "stage2_runtime".to_string(),
        json!({
            "policy_and_scope_filter": policy_filter,
            "verification_conflict_check": verification_check,
        }),
    );
    Value::Object(enriched)
}

pub async fn insert_artifact_ref(client: &Client, record: &ArtifactRefInsert<'_>) -> Result<Uuid> {
    let source_event_ids = link_surface_json_or_empty(record.source_event_ids);
    let message_refs = link_surface_json_or_empty(record.message_refs);
    let evidence_span = record.evidence_span.cloned().unwrap_or_else(|| json!({}));
    validate_artifact_ref_basis(record, &source_event_ids, &message_refs, &evidence_span)?;
    let derivation_kind = record.derivation_kind.unwrap_or("extract").to_string();
    let schema_version = record
        .schema_version
        .unwrap_or("artifact-ref-envelope-v1")
        .to_string();
    let row = client
        .query_one(
            r#"
            INSERT INTO ami.artifact_refs(
                project_id, namespace_id, artifact_kind, bucket, object_key, content_type,
                source_kind, source_event_ids, message_refs, evidence_span, derivation_kind, schema_version, metadata
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8::jsonb, $9::jsonb, $10::jsonb, $11, $12, $13)
            ON CONFLICT (bucket, object_key) DO UPDATE SET
                content_type = EXCLUDED.content_type,
                source_kind = EXCLUDED.source_kind,
                source_event_ids = EXCLUDED.source_event_ids,
                message_refs = EXCLUDED.message_refs,
                evidence_span = EXCLUDED.evidence_span,
                derivation_kind = EXCLUDED.derivation_kind,
                schema_version = EXCLUDED.schema_version,
                metadata = EXCLUDED.metadata
            RETURNING artifact_ref_id
            "#,
            &[
                &record.project_id,
                &record.namespace_id,
                &record.artifact_kind,
                &record.bucket,
                &record.object_key,
                &record.content_type,
                &record.source_kind,
                &source_event_ids,
                &message_refs,
                &evidence_span,
                &derivation_kind,
                &schema_version,
                record.metadata,
            ],
        )
        .await
        .context("failed to upsert artifact ref")?;
    Ok(row.get(0))
}

pub async fn create_artifact_ref(
    client: &Client,
    project_code: &str,
    namespace_code: &str,
    record: &ArtifactRefInsert<'_>,
) -> Result<ArtifactRefRecord> {
    let (workspace_id, project, namespace) =
        resolve_scope_ids(client, project_code, namespace_code).await?;
    let workspace = get_workspace_by_id(client, workspace_id).await?;
    let source_event_ids = link_surface_json_or_empty(record.source_event_ids);
    let message_refs = link_surface_json_or_empty(record.message_refs);
    let evidence_span = record.evidence_span.cloned().unwrap_or_else(|| json!({}));
    validate_artifact_ref_basis(record, &source_event_ids, &message_refs, &evidence_span)?;
    let derivation_kind = record.derivation_kind.unwrap_or("extract").to_string();
    let schema_version = record
        .schema_version
        .unwrap_or("artifact-ref-envelope-v1")
        .to_string();
    let policy_filter =
        run_artifact_ref_policy_scope_filter(&workspace.code, &project, &namespace, record);
    validate_artifact_ref_policy_scope_filter(&policy_filter)?;
    let verification_check = run_artifact_ref_verification_conflict_check(
        &derivation_kind,
        &source_event_ids,
        &message_refs,
        &evidence_span,
        &policy_filter,
    );
    validate_artifact_ref_verification_conflict_check(&verification_check)?;
    let stored_evidence_span = augment_artifact_ref_evidence_span_with_stage2_preflight(
        &evidence_span,
        &policy_filter,
        &verification_check,
    );
    let row = client
        .query_one(
            r#"
            INSERT INTO ami.artifact_refs(
                project_id, namespace_id, artifact_kind, bucket, object_key, content_type,
                source_kind, source_event_ids, message_refs, evidence_span, derivation_kind, schema_version, metadata
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8::jsonb, $9::jsonb, $10::jsonb, $11, $12, $13)
            ON CONFLICT (bucket, object_key) DO UPDATE SET
                content_type = EXCLUDED.content_type,
                source_kind = EXCLUDED.source_kind,
                source_event_ids = EXCLUDED.source_event_ids,
                message_refs = EXCLUDED.message_refs,
                evidence_span = EXCLUDED.evidence_span,
                derivation_kind = EXCLUDED.derivation_kind,
                schema_version = EXCLUDED.schema_version,
                metadata = EXCLUDED.metadata
            RETURNING
                artifact_ref_id,
                $14::text,
                $15::text,
                $16::text,
                artifact_kind,
                bucket,
                object_key,
                content_type,
                source_kind,
                source_event_ids,
                message_refs,
                evidence_span,
                derivation_kind,
                schema_version,
                metadata
            "#,
            &[
                &project.project_id,
                &namespace.namespace_id,
                &record.artifact_kind,
                &record.bucket,
                &record.object_key,
                &record.content_type,
                &record.source_kind,
                &source_event_ids,
                &message_refs,
                &stored_evidence_span,
                &derivation_kind,
                &schema_version,
                record.metadata,
                &workspace.code,
                &project.code,
                &namespace.code,
            ],
        )
        .await
        .with_context(|| {
            format!(
                "failed to create artifact ref for {project_code}:{namespace_code}: {}/{}",
                record.bucket, record.object_key
            )
        })?;
    Ok(artifact_ref_record_from_row(&row))
}

pub async fn get_artifact_ref(client: &Client, artifact_ref_id: Uuid) -> Result<ArtifactRefRecord> {
    let row = client
        .query_one(
            r#"
            SELECT
                ar.artifact_ref_id,
                w.code,
                p.code,
                n.code,
                ar.artifact_kind,
                ar.bucket,
                ar.object_key,
                ar.content_type,
                ar.source_kind,
                ar.source_event_ids,
                ar.message_refs,
                ar.evidence_span,
                ar.derivation_kind,
                ar.schema_version,
                ar.metadata
            FROM ami.artifact_refs ar
            INNER JOIN ami.projects p ON p.project_id = ar.project_id
            INNER JOIN ami.workspaces w ON w.workspace_id = p.workspace_id
            INNER JOIN ami.namespaces n ON n.namespace_id = ar.namespace_id
            WHERE ar.artifact_ref_id = $1
            "#,
            &[&artifact_ref_id],
        )
        .await
        .with_context(|| format!("failed to load artifact ref {}", artifact_ref_id))?;
    Ok(artifact_ref_record_from_row(&row))
}

pub async fn get_latest_memory_raw_event_for_item(
    client: &Client,
    memory_item_id: Uuid,
) -> Result<MemoryRawEventRecord> {
    let row = client
        .query_one(
            r#"
            SELECT
                mre.memory_raw_event_id,
                w.code,
                p.code,
                n.code,
                sp.code,
                mre.import_packet_id,
                mre.owner_agent_id,
                mre.event_kind,
                mre.item_kind,
                mre.visibility_scope,
                mre.sensitivity_class,
                mre.derivation_kind,
                mre.truth_state,
                mre.trust_state,
                mre.verification_state,
                mre.lifecycle_state,
                mre.identity_key,
                mre.title,
                mre.summary,
                mre.body,
                mre.source_event_ids,
                mre.artifact_refs,
                mre.message_refs,
                mre.evidence_span,
                mre.causation_id,
                mre.correlation_id,
                mre.source_epoch_ns,
                mre.source_monotonic_ns,
                mre.server_received_at_epoch_ms,
                mre.server_order_seq,
                mre.payload
            FROM ami.memory_write_outbox mwo
            INNER JOIN ami.memory_raw_events mre ON mre.memory_raw_event_id = mwo.memory_raw_event_id
            INNER JOIN ami.workspaces w ON w.workspace_id = mre.workspace_id
            INNER JOIN ami.projects p ON p.project_id = mre.project_id
            INNER JOIN ami.namespaces n ON n.namespace_id = mre.namespace_id
            LEFT JOIN ami.projects sp ON sp.project_id = mre.source_project_id
            WHERE mwo.memory_item_id = $1
            ORDER BY mre.server_order_seq DESC, mre.created_at DESC
            LIMIT 1
            "#,
            &[&memory_item_id],
        )
        .await
        .with_context(|| format!("failed to load latest memory raw event for {}", memory_item_id))?;
    Ok(memory_raw_event_record_from_row(&row))
}

pub async fn list_memory_write_outbox_for_item(
    client: &Client,
    memory_item_id: Uuid,
) -> Result<Vec<MemoryWriteOutboxRecord>> {
    let rows = client
        .query(
            r#"
            SELECT
                mwo.memory_write_outbox_id,
                w.code,
                p.code,
                n.code,
                mwo.memory_raw_event_id,
                mwo.memory_item_id,
                mwo.subject,
                mwo.delivery_kind,
                mwo.delivery_state,
                mwo.payload,
                mwo.attempt_count,
                mwo.last_error,
                mwo.published_at_epoch_ms,
                mwo.acknowledged_at_epoch_ms
            FROM ami.memory_write_outbox mwo
            INNER JOIN ami.workspaces w ON w.workspace_id = mwo.workspace_id
            INNER JOIN ami.projects p ON p.project_id = mwo.project_id
            INNER JOIN ami.namespaces n ON n.namespace_id = mwo.namespace_id
            WHERE mwo.memory_item_id = $1
            ORDER BY mwo.subject ASC, mwo.created_at ASC
            "#,
            &[&memory_item_id],
        )
        .await
        .with_context(|| format!("failed to list memory write outbox for {}", memory_item_id))?;
    Ok(rows
        .into_iter()
        .map(|row| memory_write_outbox_record_from_row(&row))
        .collect())
}

pub async fn get_context_pack(client: &Client, context_pack_id: Uuid) -> Result<ContextPackRecord> {
    let row = client
        .query_one(
            r#"
            SELECT
                cp.context_pack_id,
                p.code,
                n.code,
                cp.retrieval_mode,
                cp.query_text,
                cp.visible_projects,
                cp.payload,
                cp.artifact_ref_id,
                cp.artifact_bucket,
                cp.artifact_object_key,
                cp.artifact_state,
                cp.artifact_last_error,
                (extract(epoch from cp.artifact_updated_at) * 1000)::bigint AS artifact_updated_at_epoch_ms
            FROM ami.context_packs cp
            INNER JOIN ami.projects p ON p.project_id = cp.project_id
            INNER JOIN ami.namespaces n ON n.namespace_id = cp.namespace_id
            WHERE cp.context_pack_id = $1
            "#,
            &[&context_pack_id],
        )
        .await
        .with_context(|| format!("failed to load context pack {}", context_pack_id))?;
    Ok(context_pack_record_from_row(&row))
}

#[cfg(test)]
pub async fn insert_context_pack(client: &Client, record: &ContextPackInsert<'_>) -> Result<()> {
    client
        .execute(
            r#"
            INSERT INTO ami.context_packs(
                context_pack_id, project_id, namespace_id, retrieval_mode,
                query_text, visible_projects, payload, artifact_ref_id
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            "#,
            &[
                &record.context_pack_id,
                &record.project_id,
                &record.namespace_id,
                &record.retrieval_mode,
                &record.query_text,
                record.visible_projects,
                record.payload,
                &record.artifact_ref_id,
            ],
        )
        .await
        .context("failed to insert context pack")?;
    Ok(())
}

pub async fn create_retrieval_trace(
    client: &Client,
    record: &RetrievalTraceInsert,
) -> Result<Uuid> {
    let workspace = get_workspace_by_id(client, record.workspace_id).await?;
    let project = get_project_by_id(client, record.project_id).await?;
    let namespace = get_namespace_by_id(client, record.namespace_id).await?;
    let derivation_kind = record
        .derivation_kind
        .clone()
        .unwrap_or_else(|| "extract".to_string());
    let schema_version = record
        .schema_version
        .clone()
        .unwrap_or_else(|| "retrieval-trace-envelope-v1".to_string());
    validate_stage2_basis(
        "retrieval trace",
        &derivation_kind,
        &record.source_event_ids,
        &record.artifact_refs,
        &record.message_refs,
        &record.evidence_span,
    )?;
    let policy_filter = run_retrieval_trace_policy_scope_filter(
        client,
        &workspace.code,
        &project,
        &namespace,
        record,
    )
    .await?;
    validate_retrieval_trace_policy_scope_filter(&policy_filter)?;
    let verification_check = run_retrieval_trace_verification_conflict_check(
        &derivation_kind,
        &record.source_event_ids,
        &record.artifact_refs,
        &record.message_refs,
        &record.evidence_span,
        &record.evidence_sufficiency,
        &record.trace_payload,
        &policy_filter,
    );
    validate_retrieval_trace_verification_conflict_check(&verification_check)?;
    let stored_evidence_span = augment_retrieval_trace_evidence_span_with_stage2_preflight(
        &record.evidence_span,
        &policy_filter,
        &verification_check,
    );
    let row = client
        .query_one(
            r#"
            INSERT INTO ami.retrieval_traces(
                workspace_id,
                project_id,
                namespace_id,
                context_pack_id,
                query_text,
                requested_mode,
                effective_mode,
                scope_filter,
                candidate_summary,
                rerank_summary,
                evidence_sufficiency,
                source_kind,
                source_event_ids,
                artifact_refs,
                message_refs,
                evidence_span,
                derivation_kind,
                schema_version,
                final_decision,
                temporal_query_epoch_ms,
                trace_payload
            )
            VALUES (
                $1, $2, $3, $4, $5, $6, $7,
                $8::jsonb, $9::jsonb, $10::jsonb, $11::jsonb,
                $12, $13::jsonb, $14::jsonb, $15::jsonb, $16::jsonb, $17, $18,
                $19, $20, $21::jsonb
            )
            RETURNING retrieval_trace_id
            "#,
            &[
                &record.workspace_id,
                &record.project_id,
                &record.namespace_id,
                &record.context_pack_id,
                &record.query_text,
                &record.requested_mode,
                &record.effective_mode,
                &record.scope_filter,
                &record.candidate_summary,
                &record.rerank_summary,
                &record.evidence_sufficiency,
                &record.source_kind,
                &record.source_event_ids,
                &record.artifact_refs,
                &record.message_refs,
                &stored_evidence_span,
                &derivation_kind,
                &schema_version,
                &record.final_decision,
                &record.temporal_query_epoch_ms,
                &record.trace_payload,
            ],
        )
        .await
        .context("failed to create retrieval trace")?;
    Ok(row.get(0))
}

pub async fn get_retrieval_trace(
    client: &Client,
    retrieval_trace_id: Uuid,
) -> Result<RetrievalTraceRecord> {
    let row = client
        .query_one(
            r#"
            SELECT
                rt.retrieval_trace_id,
                w.code,
                p.code,
                n.code,
                rt.context_pack_id,
                rt.query_text,
                rt.requested_mode,
                rt.effective_mode,
                rt.scope_filter,
                rt.candidate_summary,
                rt.rerank_summary,
                rt.evidence_sufficiency,
                rt.source_kind,
                rt.source_event_ids,
                rt.artifact_refs,
                rt.message_refs,
                rt.evidence_span,
                rt.derivation_kind,
                rt.schema_version,
                rt.final_decision,
                rt.temporal_query_epoch_ms,
                rt.trace_payload
            FROM ami.retrieval_traces rt
            INNER JOIN ami.workspaces w ON w.workspace_id = rt.workspace_id
            INNER JOIN ami.projects p ON p.project_id = rt.project_id
            LEFT JOIN ami.namespaces n ON n.namespace_id = rt.namespace_id
            WHERE rt.retrieval_trace_id = $1
            "#,
            &[&retrieval_trace_id],
        )
        .await
        .with_context(|| format!("failed to load retrieval trace {}", retrieval_trace_id))?;
    Ok(RetrievalTraceRecord {
        retrieval_trace_id: row.get(0),
        workspace_code: row.get(1),
        project_code: row.get(2),
        namespace_code: row.get(3),
        context_pack_id: row.get(4),
        query_text: row.get(5),
        requested_mode: row.get(6),
        effective_mode: row.get(7),
        scope_filter: row.get(8),
        candidate_summary: row.get(9),
        rerank_summary: row.get(10),
        evidence_sufficiency: row.get(11),
        source_kind: row.get(12),
        source_event_ids: row.get(13),
        artifact_refs: row.get(14),
        message_refs: row.get(15),
        evidence_span: row.get(16),
        derivation_kind: row.get(17),
        schema_version: row.get(18),
        final_decision: row.get(19),
        temporal_query_epoch_ms: row.get(20),
        trace_payload: row.get(21),
    })
}

pub async fn create_memory_provenance(
    client: &Client,
    project_code: &str,
    namespace_code: &str,
    record: &MemoryProvenanceInsert<'_>,
) -> Result<MemoryProvenanceRecord> {
    let (workspace_id, project, namespace) =
        resolve_scope_ids(client, project_code, namespace_code).await?;
    let workspace = get_workspace_by_id(client, workspace_id).await?;
    let message_refs = record.message_refs.cloned().unwrap_or_else(|| json!([]));
    let evidence_span = record.evidence_span.cloned().unwrap_or_else(|| json!({}));
    let derivation_kind = record.derivation_kind.unwrap_or("extract");
    let schema_version = record.schema_version.unwrap_or("memory-provenance-v1");
    let trust_level = record.trust_level.unwrap_or("raw");
    let source_event_ids = match record.source_event_id {
        Some(value) => json!([value]),
        None => json!([]),
    };
    let artifact_refs = match record.artifact_ref_id {
        Some(value) => json!([format!("artifact-ref-id:{value}")]),
        None => json!([]),
    };
    validate_stage2_basis(
        "memory provenance",
        derivation_kind,
        &source_event_ids,
        &artifact_refs,
        &message_refs,
        &evidence_span,
    )?;
    let policy_filter = run_memory_provenance_policy_scope_filter(
        client,
        &workspace.code,
        &project,
        &namespace,
        record,
    )
    .await?;
    validate_memory_provenance_policy_scope_filter(&policy_filter)?;
    let verification_check = run_memory_provenance_verification_conflict_check(
        derivation_kind,
        &source_event_ids,
        &artifact_refs,
        &message_refs,
        &evidence_span,
        &policy_filter,
    );
    validate_memory_provenance_verification_conflict_check(&verification_check)?;
    let stored_evidence_span = augment_memory_provenance_evidence_span_with_stage2_preflight(
        &evidence_span,
        &policy_filter,
        &verification_check,
    );
    let row = client
        .query_one(
            r#"
            INSERT INTO ami.memory_provenance(
                workspace_id,
                project_id,
                namespace_id,
                memory_item_id,
                source_kind,
                source_event_id,
                source_snapshot_id,
                artifact_ref_id,
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
                $1, $2, $3, $4, $5, $6, $7, $8, $9,
                $10::jsonb, $11::jsonb, $12, $13, $14, $15, $16, $17, $18::jsonb
            )
            RETURNING
                memory_provenance_id,
                $19::text,
                $20::text,
                $21::text,
                memory_item_id,
                source_kind,
                source_event_id,
                source_snapshot_id,
                artifact_ref_id,
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
            "#,
            &[
                &workspace.workspace_id,
                &project.project_id,
                &namespace.namespace_id,
                &record.memory_item_id,
                &record.source_kind,
                &record.source_event_id,
                &record.source_snapshot_id,
                &record.artifact_ref_id,
                &trust_level,
                &message_refs,
                &stored_evidence_span,
                &derivation_kind,
                &record.observed_at_epoch_ms,
                &record.recorded_at_epoch_ms,
                &record.valid_from_epoch_ms,
                &record.valid_to_epoch_ms,
                &schema_version,
                record.details,
                &workspace.code,
                &project.code,
                &namespace.code,
            ],
        )
        .await
        .with_context(|| {
            format!("failed to create memory provenance for {project_code}:{namespace_code}")
        })?;
    Ok(memory_provenance_record_from_row(&row))
}

pub async fn get_memory_provenance(
    client: &Client,
    memory_provenance_id: Uuid,
) -> Result<MemoryProvenanceRecord> {
    let row = client
        .query_one(
            r#"
            SELECT
                mp.memory_provenance_id,
                w.code,
                p.code,
                n.code,
                mp.memory_item_id,
                mp.source_kind,
                mp.source_event_id,
                mp.source_snapshot_id,
                mp.artifact_ref_id,
                mp.trust_level,
                mp.message_refs,
                mp.evidence_span,
                mp.derivation_kind,
                mp.observed_at_epoch_ms,
                mp.recorded_at_epoch_ms,
                mp.valid_from_epoch_ms,
                mp.valid_to_epoch_ms,
                mp.schema_version,
                mp.details
            FROM ami.memory_provenance mp
            INNER JOIN ami.workspaces w ON w.workspace_id = mp.workspace_id
            INNER JOIN ami.projects p ON p.project_id = mp.project_id
            LEFT JOIN ami.namespaces n ON n.namespace_id = mp.namespace_id
            WHERE mp.memory_provenance_id = $1
            "#,
            &[&memory_provenance_id],
        )
        .await
        .with_context(|| format!("failed to load memory provenance {}", memory_provenance_id))?;
    Ok(memory_provenance_record_from_row(&row))
}

pub async fn create_restore_pack(
    client: &Client,
    project_code: &str,
    namespace_code: &str,
    record: &RestorePackInsert<'_>,
) -> Result<RestorePackRecord> {
    create_restore_pack_detailed(client, project_code, namespace_code, record)
        .await
        .map_err(|error| error.error)
}

pub(crate) async fn create_restore_pack_detailed(
    client: &Client,
    project_code: &str,
    namespace_code: &str,
    record: &RestorePackInsert<'_>,
) -> std::result::Result<RestorePackRecord, RestorePackCreateError> {
    let (workspace_id, project, namespace) =
        resolve_scope_ids(client, project_code, namespace_code)
            .await
            .map_err(|error| {
                RestorePackCreateError::before_write(
                    project_code,
                    namespace_code,
                    record.pack_kind,
                    record.source_snapshot_id,
                    error,
                )
            })?;
    let workspace = get_workspace_by_id(client, workspace_id)
        .await
        .map_err(|error| {
            RestorePackCreateError::before_write(
                &project.code,
                &namespace.code,
                record.pack_kind,
                record.source_snapshot_id,
                error,
            )
        })?;
    let source_event_ids = record
        .source_event_ids
        .cloned()
        .unwrap_or_else(|| json!([]));
    let artifact_refs = record.artifact_refs.cloned().unwrap_or_else(|| json!([]));
    let message_refs = record.message_refs.cloned().unwrap_or_else(|| json!([]));
    let evidence_span = record.evidence_span.cloned().unwrap_or_else(|| json!({}));
    let derivation_kind = record.derivation_kind.unwrap_or("summary");
    let schema_version = record.schema_version.unwrap_or("restore-pack-envelope-v1");
    validate_stage2_basis(
        "restore pack",
        derivation_kind,
        &source_event_ids,
        &artifact_refs,
        &message_refs,
        &evidence_span,
    )
    .map_err(|error| {
        RestorePackCreateError::before_write(
            &project.code,
            &namespace.code,
            record.pack_kind,
            record.source_snapshot_id,
            error,
        )
    })?;
    let policy_filter =
        run_restore_pack_policy_scope_filter(client, &workspace.code, &project, &namespace, record)
            .await
            .map_err(|error| {
                RestorePackCreateError::before_write(
                    &project.code,
                    &namespace.code,
                    record.pack_kind,
                    record.source_snapshot_id,
                    error,
                )
            })?;
    validate_restore_pack_policy_scope_filter(&policy_filter).map_err(|error| {
        RestorePackCreateError::before_write(
            &project.code,
            &namespace.code,
            record.pack_kind,
            record.source_snapshot_id,
            error,
        )
    })?;
    let verification_check = run_restore_pack_verification_conflict_check(
        derivation_kind,
        &source_event_ids,
        &artifact_refs,
        &message_refs,
        &evidence_span,
        &policy_filter,
    );
    validate_restore_pack_verification_conflict_check(&verification_check).map_err(|error| {
        RestorePackCreateError::before_write(
            &project.code,
            &namespace.code,
            record.pack_kind,
            record.source_snapshot_id,
            error,
        )
    })?;
    let stored_evidence_span = augment_restore_pack_evidence_span_with_stage2_preflight(
        &evidence_span,
        &policy_filter,
        &verification_check,
    );
    if let Some(source_snapshot_id) = record.source_snapshot_id {
        let advisory_lock_key = restore_pack_source_snapshot_advisory_lock_key(
            namespace.namespace_id,
            record.pack_kind,
            source_snapshot_id,
        );
        return with_postgres_advisory_lock(
            client,
            advisory_lock_key,
            format!(
                "failed to acquire restore pack advisory lock for {project_code}:{namespace_code}"
            ),
            format!(
                "failed to release restore pack advisory lock for {project_code}:{namespace_code}"
            ),
            || async {
                if let Some(existing_row) = client
                    .query_opt(
                        r#"
                        SELECT
                            rp.restore_pack_id,
                            w.code,
                            p.code,
                            n.code,
                            rp.agent_scope,
                            rp.session_id,
                            rp.thread_id,
                            rp.source_snapshot_id,
                            rp.pack_kind,
                            rp.source_kind,
                            rp.source_event_ids,
                            rp.artifact_refs,
                            rp.message_refs,
                            rp.evidence_span,
                            rp.derivation_kind,
                            rp.schema_version,
                            rp.headline,
                            rp.summary,
                            rp.payload,
                            rp.captured_at_epoch_ms
                        FROM ami.restore_packs rp
                        INNER JOIN ami.workspaces w ON w.workspace_id = rp.workspace_id
                        INNER JOIN ami.projects p ON p.project_id = rp.project_id
                        LEFT JOIN ami.namespaces n ON n.namespace_id = rp.namespace_id
                        WHERE rp.project_id = $1
                          AND rp.namespace_id = $2
                          AND rp.pack_kind = $3
                          AND rp.source_snapshot_id = $4
                        ORDER BY rp.captured_at_epoch_ms DESC NULLS LAST, rp.created_at DESC, rp.restore_pack_id DESC
                        LIMIT 1
                        "#,
                        &[
                            &project.project_id,
                            &namespace.namespace_id,
                            &record.pack_kind,
                            &source_snapshot_id,
                        ],
                    )
                    .await
                    .with_context(|| {
                        format!(
                            "failed to inspect existing restore pack for {project_code}:{namespace_code}"
                        )
                    })?
                {
                    let existing_record = restore_pack_record_from_row(&existing_row);
                    validate_restore_pack_record_source_identity(&existing_record)?;
                    ensure_existing_restore_pack_matches_incoming(
                        &existing_record,
                        record,
                        &source_event_ids,
                        &artifact_refs,
                        &message_refs,
                        &stored_evidence_span,
                        derivation_kind,
                        schema_version,
                    )?;
                    return Ok(existing_record);
                }
                #[cfg(test)]
                if let Some(forced_error) = forced_restore_pack_create_error_for_tests(
                    &project.code,
                    &namespace.code,
                    record.pack_kind,
                    Some(source_snapshot_id),
                    false,
                ) {
                    return Err(forced_error.error);
                }
                #[cfg(test)]
                maybe_delay_restore_pack_create_after_lookup_for_tests().await;
                let row = create_restore_pack_row(
                    client,
                    workspace_id,
                    &workspace.code,
                    &project,
                    &namespace,
                    record,
                    &source_event_ids,
                    &artifact_refs,
                    &message_refs,
                    &stored_evidence_span,
                    derivation_kind,
                    schema_version,
                )
                .await?;
                #[cfg(test)]
                if let Some(forced_error) = forced_restore_pack_create_error_for_tests(
                    &project.code,
                    &namespace.code,
                    record.pack_kind,
                    Some(source_snapshot_id),
                    true,
                ) {
                    return Err(forced_error.error);
                }
                Ok(row)
            },
        )
        .await
        .map_err(|error| {
            let forced_after_write =
                std::env::var("AMAI_TEST_FORCE_RESTORE_PACK_CREATE_FAILURE")
                    .ok()
                    .as_deref()
                    == Some("outcome_unknown_after_write");
            if forced_after_write {
                RestorePackCreateError::outcome_unknown_after_write(
                    &project.code,
                    &namespace.code,
                    record.pack_kind,
                    Some(source_snapshot_id),
                    error,
                )
            } else {
                RestorePackCreateError::before_write(
                    &project.code,
                    &namespace.code,
                    record.pack_kind,
                    Some(source_snapshot_id),
                    error,
                )
            }
        });
    }
    #[cfg(test)]
    if let Some(forced_error) = forced_restore_pack_create_error_for_tests(
        &project.code,
        &namespace.code,
        record.pack_kind,
        record.source_snapshot_id,
        false,
    ) {
        return Err(forced_error);
    }
    let row = create_restore_pack_row(
        client,
        workspace_id,
        &workspace.code,
        &project,
        &namespace,
        record,
        &source_event_ids,
        &artifact_refs,
        &message_refs,
        &stored_evidence_span,
        derivation_kind,
        schema_version,
    )
    .await
    .map_err(|error| {
        RestorePackCreateError::before_write(
            &project.code,
            &namespace.code,
            record.pack_kind,
            record.source_snapshot_id,
            error,
        )
    })?;
    #[cfg(test)]
    if let Some(forced_error) = forced_restore_pack_create_error_for_tests(
        &project.code,
        &namespace.code,
        record.pack_kind,
        record.source_snapshot_id,
        true,
    ) {
        return Err(forced_error);
    }
    Ok(row)
}

pub(crate) async fn lookup_restore_pack_by_source_snapshot_id(
    client: &Client,
    project_id: Uuid,
    namespace_id: Uuid,
    pack_kind: &str,
    source_snapshot_id: Uuid,
) -> Result<Option<RestorePackRecord>> {
    client
        .query_opt(
            r#"
            SELECT
                rp.restore_pack_id,
                w.code,
                p.code,
                n.code,
                rp.agent_scope,
                rp.session_id,
                rp.thread_id,
                rp.source_snapshot_id,
                rp.pack_kind,
                rp.source_kind,
                rp.source_event_ids,
                rp.artifact_refs,
                rp.message_refs,
                rp.evidence_span,
                rp.derivation_kind,
                rp.schema_version,
                rp.headline,
                rp.summary,
                rp.payload,
                rp.captured_at_epoch_ms
            FROM ami.restore_packs rp
            INNER JOIN ami.workspaces w ON w.workspace_id = rp.workspace_id
            INNER JOIN ami.projects p ON p.project_id = rp.project_id
            LEFT JOIN ami.namespaces n ON n.namespace_id = rp.namespace_id
            WHERE rp.project_id = $1
              AND rp.namespace_id = $2
              AND rp.pack_kind = $3
              AND rp.source_snapshot_id = $4
            ORDER BY rp.captured_at_epoch_ms DESC NULLS LAST, rp.created_at DESC, rp.restore_pack_id DESC
            LIMIT 1
            "#,
            &[&project_id, &namespace_id, &pack_kind, &source_snapshot_id],
        )
        .await
        .context("failed to lookup restore pack by source_snapshot_id")
        .and_then(|row| {
            row.map(|row| {
                let record = restore_pack_record_from_row(&row);
                validate_restore_pack_record_source_identity(&record)?;
                Ok(record)
            })
            .transpose()
        })
}

pub async fn get_restore_pack(client: &Client, restore_pack_id: Uuid) -> Result<RestorePackRecord> {
    let row = client
        .query_one(
            r#"
            SELECT
                rp.restore_pack_id,
                w.code,
                p.code,
                n.code,
                rp.agent_scope,
                rp.session_id,
                rp.thread_id,
                rp.source_snapshot_id,
                rp.pack_kind,
                rp.source_kind,
                rp.source_event_ids,
                rp.artifact_refs,
                rp.message_refs,
                rp.evidence_span,
                rp.derivation_kind,
                rp.schema_version,
                rp.headline,
                rp.summary,
                rp.payload,
                rp.captured_at_epoch_ms
            FROM ami.restore_packs rp
            INNER JOIN ami.workspaces w ON w.workspace_id = rp.workspace_id
            INNER JOIN ami.projects p ON p.project_id = rp.project_id
            LEFT JOIN ami.namespaces n ON n.namespace_id = rp.namespace_id
            WHERE rp.restore_pack_id = $1
            "#,
            &[&restore_pack_id],
        )
        .await
        .with_context(|| format!("failed to load restore pack {}", restore_pack_id))?;
    let record = restore_pack_record_from_row(&row);
    validate_restore_pack_record_source_identity(&record)?;
    Ok(record)
}

pub async fn insert_context_pack_pending_artifact(
    client: &Client,
    record: &ContextPackInsert<'_>,
    bucket: &str,
    object_key: &str,
) -> Result<()> {
    client
        .execute(
            r#"
            INSERT INTO ami.context_packs(
                context_pack_id, project_id, namespace_id, retrieval_mode,
                query_text, visible_projects, payload, artifact_ref_id,
                artifact_bucket, artifact_object_key, artifact_state, artifact_last_error, artifact_updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, NULL, $8, $9, 'pending', NULL, now())
            "#,
            &[
                &record.context_pack_id,
                &record.project_id,
                &record.namespace_id,
                &record.retrieval_mode,
                &record.query_text,
                record.visible_projects,
                record.payload,
                &bucket,
                &object_key,
            ],
        )
        .await
        .context("failed to insert pending context pack")?;
    Ok(())
}

pub async fn claim_pending_context_pack_artifacts(
    client: &Client,
    limit: i64,
) -> Result<Vec<PendingContextPackArtifactRecord>> {
    let rows = client
        .query(
            r#"
            WITH claimed AS (
                SELECT context_pack_id
                FROM ami.context_packs
                WHERE artifact_state IN ('pending', 'failed')
                  AND artifact_bucket IS NOT NULL
                  AND artifact_object_key IS NOT NULL
                ORDER BY created_at ASC
                LIMIT $1
                FOR UPDATE SKIP LOCKED
            )
            UPDATE ami.context_packs cp
            SET artifact_state = 'materializing',
                artifact_last_error = NULL,
                artifact_updated_at = now()
            FROM claimed
            WHERE cp.context_pack_id = claimed.context_pack_id
            RETURNING cp.context_pack_id, cp.project_id, cp.namespace_id,
                      cp.artifact_bucket, cp.artifact_object_key, cp.payload
            "#,
            &[&limit],
        )
        .await
        .context("failed to claim pending context pack artifacts")?;

    Ok(rows
        .into_iter()
        .map(|row| PendingContextPackArtifactRecord {
            context_pack_id: row.get(0),
            project_id: row.get(1),
            namespace_id: row.get(2),
            bucket: row.get(3),
            object_key: row.get(4),
            payload: row.get(5),
        })
        .collect())
}

pub async fn list_pending_context_pack_artifacts(
    client: &Client,
    limit: i64,
    context_pack_id: Option<Uuid>,
) -> Result<Vec<PendingContextPackArtifactRecord>> {
    let rows = client
        .query(
            r#"
            SELECT
                context_pack_id,
                project_id,
                namespace_id,
                artifact_bucket,
                artifact_object_key,
                payload
            FROM ami.context_packs
            WHERE artifact_state IN ('pending', 'failed')
              AND artifact_bucket IS NOT NULL
              AND artifact_object_key IS NOT NULL
              AND ($2::uuid IS NULL OR context_pack_id = $2)
            ORDER BY created_at ASC
            LIMIT $1
            "#,
            &[&limit, &context_pack_id],
        )
        .await
        .context("failed to list pending context pack artifacts")?;
    Ok(rows
        .into_iter()
        .map(|row| PendingContextPackArtifactRecord {
            context_pack_id: row.get(0),
            project_id: row.get(1),
            namespace_id: row.get(2),
            bucket: row.get(3),
            object_key: row.get(4),
            payload: row.get(5),
        })
        .collect())
}

pub async fn mark_context_pack_artifacts_materialized(
    client: &Client,
    bucket: &str,
    object_key: &str,
    artifact_ref_id: Uuid,
) -> Result<u64> {
    client
        .execute(
            r#"
            UPDATE ami.context_packs
            SET artifact_ref_id = $3,
                artifact_state = 'materialized',
                artifact_last_error = NULL,
                artifact_updated_at = now()
            WHERE artifact_bucket = $1
              AND artifact_object_key = $2
            "#,
            &[&bucket, &object_key, &artifact_ref_id],
        )
        .await
        .context("failed to mark context pack artifacts materialized")
}

pub async fn mark_context_pack_artifact_failed(
    client: &Client,
    context_pack_id: Uuid,
    error_text: &str,
) -> Result<()> {
    client
        .execute(
            r#"
            UPDATE ami.context_packs
            SET artifact_state = 'failed',
                artifact_last_error = $2,
                artifact_updated_at = now()
            WHERE context_pack_id = $1
            "#,
            &[&context_pack_id, &error_text],
        )
        .await
        .context("failed to mark context pack artifact failed")?;
    Ok(())
}
