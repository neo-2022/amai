use super::*;

pub async fn create_import_packet(
    client: &Client,
    source_project_code: &str,
    target_project_code: &str,
    transfer_policy_code: Option<&str>,
    requested_by_agent_code: Option<&str>,
    status: &str,
    summary: Option<&str>,
    reason: Option<&str>,
    imported_by_agent_scope: &str,
    trust_state: &str,
    verification_state: &str,
    borrowed_status: &str,
    can_promote_after_verification: bool,
    memory_object_ids: &[String],
    artifact_refs: &[String],
    source_kind: Option<&str>,
    source_event_ids: Option<&Value>,
    message_refs: Option<&Value>,
    evidence_span: Option<&Value>,
    derivation_kind: Option<&str>,
    schema_version: Option<&str>,
) -> Result<ImportPacketRecord> {
    let source = get_project_by_code(client, source_project_code).await?;
    let target = get_project_by_code(client, target_project_code).await?;
    let workspace_id = get_project_workspace_id(client, source.project_id).await?;
    let relation = find_project_link_context(client, source.project_id, target.project_id).await?;
    let (project_link_present, requires_approval, relation_transfer_policy_id) = match relation {
        Some((
            _,
            _project_link_type,
            requires_approval,
            relation_transfer_policy_id,
            _workspace_id,
        )) => (true, requires_approval, relation_transfer_policy_id),
        None => (false, false, None),
    };
    let transfer_policy = match transfer_policy_code {
        Some(code) => find_transfer_policy_by_code(client, code).await?,
        None => match relation_transfer_policy_id {
            Some(policy_id) => {
                let row = client
                    .query_one(
                        r#"
                        SELECT
                            tp.transfer_policy_id,
                            w.code,
                            tp.code,
                            tp.display_name,
                            tp.default_decision,
                            tp.allow_cross_project_read,
                            tp.allow_import,
                            tp.allow_verified_writeback,
                            tp.requires_human_approval
                        FROM ami.transfer_policies tp
                        INNER JOIN ami.workspaces w ON w.workspace_id = tp.workspace_id
                        WHERE tp.transfer_policy_id = $1
                        "#,
                        &[&policy_id],
                    )
                    .await
                    .context("failed to load relation transfer policy")?;
                Some(transfer_policy_record_from_row(&row))
            }
            None => None,
        },
    };
    let requested_by_agent_id = match requested_by_agent_code {
        Some(code) => find_agent_id_by_code(client, code).await?,
        None => None,
    };
    let transfer_policy_allows_import = transfer_policy
        .as_ref()
        .is_none_or(|policy| policy.allow_import);
    let access_policy_import_granted = ensure_cross_project_policy_access(
        client,
        workspace_id,
        source.project_id,
        "fact",
        &["cross_project_linked", "org_global"],
        AccessPolicyAction::Import,
        &format!("import {} -> {}", source.code, target.code),
    )
    .await
    .is_ok();
    let policy_filter = ImportPacketPolicyScopeFilter {
        source_project_code: source.code.clone(),
        target_project_code: target.code.clone(),
        project_link_present,
        transfer_policy_code: transfer_policy.as_ref().map(|item| item.code.clone()),
        transfer_policy_allows_import,
        access_policy_import_granted,
        approval_required: requires_approval,
        verified_import_blocked_until_approval: requires_approval && status == "verified",
        scope_allowed: project_link_present
            && transfer_policy_allows_import
            && access_policy_import_granted
            && !(requires_approval && status == "verified"),
    };
    validate_import_packet_policy_scope_filter(&policy_filter)?;
    let transfer_policy_id = transfer_policy.as_ref().map(|item| item.transfer_policy_id);
    let transfer_policy_code_value = transfer_policy.as_ref().map(|item| item.code.clone());
    let requested_by_agent_code_value = requested_by_agent_code.map(str::to_string);
    let memory_object_ids_value = Value::Array(
        memory_object_ids
            .iter()
            .cloned()
            .map(Value::String)
            .collect(),
    );
    let artifact_refs_value =
        Value::Array(artifact_refs.iter().cloned().map(Value::String).collect());
    let source_event_ids_value = source_event_ids.cloned().unwrap_or_else(|| json!([]));
    let message_refs_value = message_refs.cloned().unwrap_or_else(|| json!([]));
    let evidence_span_value = evidence_span.cloned().unwrap_or_else(|| json!({}));
    let derivation_kind_value = derivation_kind.unwrap_or("extract");
    let schema_version_value = schema_version.unwrap_or("import-packet-envelope-v1");
    validate_stage2_basis(
        "import packet",
        derivation_kind_value,
        &source_event_ids_value,
        &artifact_refs_value,
        &message_refs_value,
        &evidence_span_value,
    )?;
    let verification_check = ImportPacketVerificationConflictCheck {
        evidence_present: derivation_kind_value == "operator_write"
            || !source_event_ids_value
                .as_array()
                .unwrap_or(&vec![])
                .is_empty()
            || !artifact_refs_value.as_array().unwrap_or(&vec![]).is_empty()
            || !message_refs_value.as_array().unwrap_or(&vec![]).is_empty()
            || evidence_span_value
                .as_object()
                .is_some_and(|span| !span.is_empty()),
        poisoned_detected: import_packet_marks_poisoned(&evidence_span_value),
        same_project_conflict: source.project_id == target.project_id,
        write_allowed: !import_packet_marks_poisoned(&evidence_span_value)
            && source.project_id != target.project_id
            && (derivation_kind_value == "operator_write"
                || !source_event_ids_value
                    .as_array()
                    .unwrap_or(&vec![])
                    .is_empty()
                || !artifact_refs_value.as_array().unwrap_or(&vec![]).is_empty()
                || !message_refs_value.as_array().unwrap_or(&vec![]).is_empty()
                || evidence_span_value
                    .as_object()
                    .is_some_and(|span| !span.is_empty())),
    };
    validate_import_packet_verification_conflict_check(&verification_check)?;
    let stored_evidence_span = augment_import_packet_evidence_span_with_stage2_preflight(
        &evidence_span_value,
        &policy_filter,
        &verification_check,
    );
    let row = client
        .query_one(
            r#"
            INSERT INTO ami.import_packets(
                source_project_id,
                target_project_id,
                transfer_policy_id,
                requested_by_agent_id,
                status,
                summary,
                allowed_by_project_link,
                memory_object_ids,
                artifact_refs,
                reason,
                imported_by_agent_scope,
                imported_at,
                trust_state,
                verification_state,
                borrowed_status,
                can_promote_after_verification,
                source_kind,
                source_event_ids,
                message_refs,
                evidence_span,
                derivation_kind,
                schema_version
            )
            VALUES ($1, $2, $3, $4, $5, $6, TRUE, $7::jsonb, $8::jsonb, $9, $10, now(), $11, $12, $13, $14, $15, $16::jsonb, $17::jsonb, $18::jsonb, $19, $20)
            RETURNING
                import_packet_id,
                $21::text,
                $22::text,
                $23::text,
                $24::text,
                source_kind,
                source_event_ids,
                artifact_refs,
                message_refs,
                evidence_span,
                derivation_kind,
                schema_version,
                status,
                summary,
                allowed_by_project_link,
                reason,
                imported_by_agent_scope,
                trust_state,
                verification_state,
                borrowed_status,
                can_promote_after_verification,
                to_char(created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"')
            "#,
            &[
                &source.project_id,
                &target.project_id,
                &transfer_policy_id,
                &requested_by_agent_id,
                &status,
                &summary,
                &memory_object_ids_value,
                &artifact_refs_value,
                &reason,
                &imported_by_agent_scope,
                &trust_state,
                &verification_state,
                &borrowed_status,
                &can_promote_after_verification,
                &source_kind,
                &source_event_ids_value,
                &message_refs_value,
                &stored_evidence_span,
                &derivation_kind_value,
                &schema_version_value,
                &source.code,
                &target.code,
                &transfer_policy_code_value,
                &requested_by_agent_code_value,
            ],
        )
        .await
        .context("failed to create import packet")?;
    Ok(ImportPacketRecord {
        import_packet_id: row.get(0),
        source_project_code: row.get(1),
        target_project_code: row.get(2),
        transfer_policy_code: row.get(3),
        requested_by_agent_code: row.get(4),
        source_kind: row.get(5),
        source_event_ids: row.get(6),
        artifact_refs: row.get(7),
        message_refs: row.get(8),
        evidence_span: row.get(9),
        derivation_kind: row.get(10),
        schema_version: row.get(11),
        status: row.get(12),
        summary: row.get(13),
        allowed_by_project_link: row.get(14),
        reason: row.get(15),
        imported_by_agent_scope: row.get(16),
        trust_state: row.get(17),
        verification_state: row.get(18),
        borrowed_status: row.get(19),
        can_promote_after_verification: row.get(20),
        created_at: row.get(21),
    })
}

pub async fn list_import_packets(
    client: &Client,
    project_code: Option<&str>,
    import_packet_id: Option<Uuid>,
) -> Result<Vec<ImportPacketRecord>> {
    let rows = client
        .query(
            r#"
            SELECT
                ip.import_packet_id,
                source_project.code,
                target_project.code,
                tp.code,
                agent.code,
                ip.source_kind,
                ip.source_event_ids,
                ip.artifact_refs,
                ip.message_refs,
                ip.evidence_span,
                ip.derivation_kind,
                ip.schema_version,
                ip.status,
                ip.summary,
                ip.allowed_by_project_link,
                ip.reason,
                ip.imported_by_agent_scope,
                ip.trust_state,
                ip.verification_state,
                ip.borrowed_status,
                ip.can_promote_after_verification,
                to_char(ip.created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"')
            FROM ami.import_packets ip
            INNER JOIN ami.projects source_project ON source_project.project_id = ip.source_project_id
            INNER JOIN ami.projects target_project ON target_project.project_id = ip.target_project_id
            LEFT JOIN ami.transfer_policies tp ON tp.transfer_policy_id = ip.transfer_policy_id
            LEFT JOIN ami.agents agent ON agent.agent_id = ip.requested_by_agent_id
            WHERE ($1::text IS NULL OR source_project.code = $1 OR target_project.code = $1)
              AND ($2::uuid IS NULL OR ip.import_packet_id = $2)
            ORDER BY ip.created_at DESC, ip.import_packet_id DESC
            "#,
            &[&project_code, &import_packet_id],
        )
        .await
        .context("failed to list import packets")?;
    Ok(rows
        .into_iter()
        .map(|row| ImportPacketRecord {
            import_packet_id: row.get(0),
            source_project_code: row.get(1),
            target_project_code: row.get(2),
            transfer_policy_code: row.get(3),
            requested_by_agent_code: row.get(4),
            source_kind: row.get(5),
            source_event_ids: row.get(6),
            artifact_refs: row.get(7),
            message_refs: row.get(8),
            evidence_span: row.get(9),
            derivation_kind: row.get(10),
            schema_version: row.get(11),
            status: row.get(12),
            summary: row.get(13),
            allowed_by_project_link: row.get(14),
            reason: row.get(15),
            imported_by_agent_scope: row.get(16),
            trust_state: row.get(17),
            verification_state: row.get(18),
            borrowed_status: row.get(19),
            can_promote_after_verification: row.get(20),
            created_at: row.get(21),
        })
        .collect())
}

pub async fn get_import_packet(
    client: &Client,
    import_packet_id: Uuid,
) -> Result<ImportPacketRecord> {
    let row = client
        .query_one(
            r#"
            SELECT
                ip.import_packet_id,
                source_project.code,
                target_project.code,
                tp.code,
                agent.code,
                ip.source_kind,
                ip.source_event_ids,
                ip.artifact_refs,
                ip.message_refs,
                ip.evidence_span,
                ip.derivation_kind,
                ip.schema_version,
                ip.status,
                ip.summary,
                ip.allowed_by_project_link,
                ip.reason,
                ip.imported_by_agent_scope,
                ip.trust_state,
                ip.verification_state,
                ip.borrowed_status,
                ip.can_promote_after_verification,
                to_char(ip.created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"')
            FROM ami.import_packets ip
            INNER JOIN ami.projects source_project ON source_project.project_id = ip.source_project_id
            INNER JOIN ami.projects target_project ON target_project.project_id = ip.target_project_id
            LEFT JOIN ami.transfer_policies tp ON tp.transfer_policy_id = ip.transfer_policy_id
            LEFT JOIN ami.agents agent ON agent.agent_id = ip.requested_by_agent_id
            WHERE ip.import_packet_id = $1
            "#,
            &[&import_packet_id],
        )
        .await
        .with_context(|| format!("failed to load import packet {}", import_packet_id))?;
    Ok(ImportPacketRecord {
        import_packet_id: row.get(0),
        source_project_code: row.get(1),
        target_project_code: row.get(2),
        transfer_policy_code: row.get(3),
        requested_by_agent_code: row.get(4),
        source_kind: row.get(5),
        source_event_ids: row.get(6),
        artifact_refs: row.get(7),
        message_refs: row.get(8),
        evidence_span: row.get(9),
        derivation_kind: row.get(10),
        schema_version: row.get(11),
        status: row.get(12),
        summary: row.get(13),
        allowed_by_project_link: row.get(14),
        reason: row.get(15),
        imported_by_agent_scope: row.get(16),
        trust_state: row.get(17),
        verification_state: row.get(18),
        borrowed_status: row.get(19),
        can_promote_after_verification: row.get(20),
        created_at: row.get(21),
    })
}

pub struct ImportPacketUpdate<'a> {
    pub import_packet_id: Uuid,
    pub status: Option<&'a str>,
    pub summary: Option<&'a str>,
    pub reason: Option<&'a str>,
    pub imported_by_agent_scope: Option<&'a str>,
    pub trust_state: Option<&'a str>,
    pub verification_state: Option<&'a str>,
    pub borrowed_status: Option<&'a str>,
    pub can_promote_after_verification: Option<bool>,
    pub actor_agent_code: Option<&'a str>,
}

async fn load_import_packet_transfer_policy_for_writeback(
    client: &Client,
    packet_transfer_policy_id: Option<Uuid>,
    relation_transfer_policy_id: Option<Uuid>,
) -> Result<Option<TransferPolicyRecord>> {
    let policy_id = packet_transfer_policy_id.or(relation_transfer_policy_id);
    let Some(policy_id) = policy_id else {
        return Ok(None);
    };
    let row = client
        .query_one(
            r#"
            SELECT
                tp.transfer_policy_id,
                w.code,
                tp.code,
                tp.display_name,
                tp.default_decision,
                tp.allow_cross_project_read,
                tp.allow_import,
                tp.allow_verified_writeback,
                tp.requires_human_approval
            FROM ami.transfer_policies tp
            INNER JOIN ami.workspaces w ON w.workspace_id = tp.workspace_id
            WHERE tp.transfer_policy_id = $1
            "#,
            &[&policy_id],
        )
        .await
        .with_context(|| format!("failed to load transfer policy {}", policy_id))?;
    Ok(Some(transfer_policy_record_from_row(&row)))
}

pub async fn update_import_packet(
    client: &Client,
    update: ImportPacketUpdate<'_>,
) -> Result<ImportPacketRecord> {
    if update.status == Some("quarantined") && update.reason.is_none() {
        return Err(anyhow!(
            "import packet {} cannot be quarantined without reason",
            update.import_packet_id
        ));
    }
    let context = client
        .query_one(
            r#"
            SELECT
                source_project_id,
                target_project_id,
                (
                    SELECT workspace_id
                    FROM ami.projects p
                    WHERE p.project_id = source_project_id
                ) AS workspace_id,
                transfer_policy_id,
                can_promote_after_verification,
                verification_state
            FROM ami.import_packets
            WHERE import_packet_id = $1
            "#,
            &[&update.import_packet_id],
        )
        .await
        .context("failed to load import packet update context")?;
    let source_project_id: Uuid = context.get(0);
    let target_project_id: Uuid = context.get(1);
    let workspace_id: Uuid = context.get(2);
    let packet_transfer_policy_id: Option<Uuid> = context.get(3);
    let current_can_promote: bool = context.get(4);
    let current_verification_state: String = context.get(5);
    if update.status == Some("verified") || update.borrowed_status == Some("verified_local_copy") {
        let effective_can_promote = update
            .can_promote_after_verification
            .unwrap_or(current_can_promote);
        let effective_verification_state = update
            .verification_state
            .unwrap_or(current_verification_state.as_str());
        if !effective_can_promote {
            return Err(anyhow!(
                "import packet {} cannot be promoted without can_promote_after_verification",
                update.import_packet_id
            ));
        }
        if effective_verification_state != "verified" {
            return Err(anyhow!(
                "import packet {} cannot be promoted without verification_state=verified",
                update.import_packet_id
            ));
        }
        let link = find_project_link_context(client, source_project_id, target_project_id).await?;
        let Some((
            _relation_type,
            _project_link_type,
            requires_approval,
            relation_transfer_policy_id,
            _workspace_id,
        )) = link
        else {
            return Err(anyhow!(
                "import packet {} cannot be promoted after project_link revoke",
                update.import_packet_id
            ));
        };
        if requires_approval {
            return Err(anyhow!(
                "import packet {} cannot be promoted while relation still requires approval",
                update.import_packet_id
            ));
        }
        let transfer_policy = load_import_packet_transfer_policy_for_writeback(
            client,
            packet_transfer_policy_id,
            relation_transfer_policy_id,
        )
        .await?;
        if transfer_policy
            .as_ref()
            .is_some_and(|policy| !policy.allow_verified_writeback)
        {
            return Err(anyhow!(
                "import packet {} cannot be promoted because transfer policy {:?} blocks verified writeback",
                update.import_packet_id,
                transfer_policy.as_ref().map(|policy| policy.code.as_str())
            ));
        }
        if transfer_policy
            .as_ref()
            .is_some_and(|policy| policy.requires_human_approval)
        {
            return Err(anyhow!(
                "import packet {} cannot be promoted because transfer policy {:?} still requires human approval",
                update.import_packet_id,
                transfer_policy.as_ref().map(|policy| policy.code.as_str())
            ));
        }
        ensure_cross_project_policy_access(
            client,
            workspace_id,
            source_project_id,
            "fact",
            &["cross_project_linked", "imported", "org_global"],
            AccessPolicyAction::Promote,
            &format!("promote import packet {}", update.import_packet_id),
        )
        .await?;
        ensure_cross_project_policy_access(
            client,
            workspace_id,
            source_project_id,
            "fact",
            &["cross_project_linked", "imported", "org_global"],
            AccessPolicyAction::ApproveTransfer,
            &format!("approve import packet {}", update.import_packet_id),
        )
        .await?;
    }
    let actor_agent_id = match update.actor_agent_code {
        Some(code) => find_agent_id_by_code(client, code).await?,
        None => None,
    };
    let row = client
        .query_one(
            r#"
            UPDATE ami.import_packets ip
            SET status = COALESCE($2, ip.status),
                summary = COALESCE($3, ip.summary),
                reason = COALESCE($4, ip.reason),
                imported_by_agent_scope = COALESCE($5, ip.imported_by_agent_scope),
                trust_state = COALESCE($6, ip.trust_state),
                verification_state = COALESCE($7, ip.verification_state),
                borrowed_status = COALESCE($8, ip.borrowed_status),
                can_promote_after_verification = COALESCE($9, ip.can_promote_after_verification),
                updated_by_agent_id = COALESCE($10, ip.updated_by_agent_id),
                override_reason = COALESCE($11, ip.override_reason),
                updated_at = now()
            FROM ami.projects source_project,
                 ami.projects target_project,
                 ami.projects workspace_project
            WHERE ip.import_packet_id = $1
              AND source_project.project_id = ip.source_project_id
              AND target_project.project_id = ip.target_project_id
              AND workspace_project.project_id = ip.source_project_id
            RETURNING
                ip.import_packet_id,
                source_project.code,
                target_project.code,
                (
                    SELECT tp.code
                    FROM ami.transfer_policies tp
                    WHERE tp.transfer_policy_id = ip.transfer_policy_id
                ),
                (
                    SELECT a.code
                    FROM ami.agents a
                    WHERE a.agent_id = ip.requested_by_agent_id
                ),
                ip.source_kind,
                ip.source_event_ids,
                ip.artifact_refs,
                ip.message_refs,
                ip.evidence_span,
                ip.derivation_kind,
                ip.schema_version,
                ip.status,
                ip.summary,
                ip.allowed_by_project_link,
                ip.reason,
                ip.imported_by_agent_scope,
                ip.trust_state,
                ip.verification_state,
                ip.borrowed_status,
                ip.can_promote_after_verification,
                to_char(ip.created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),
                workspace_project.workspace_id
            "#,
            &[
                &update.import_packet_id,
                &update.status,
                &update.summary,
                &update.reason,
                &update.imported_by_agent_scope,
                &update.trust_state,
                &update.verification_state,
                &update.borrowed_status,
                &update.can_promote_after_verification,
                &actor_agent_id,
                &update.reason,
            ],
        )
        .await
        .context("failed to update import packet")?;
    let workspace_id: Uuid = row.get(22);
    let returned_source_event_ids: Value = row.get(6);
    let returned_artifact_refs: Value = row.get(7);
    let returned_message_refs: Value = row.get(8);
    let returned_evidence_span: Value = row.get(9);
    let returned_status: String = row.get(12);
    let target_project_code: String = row.get(2);
    if let Some(reason) = update.reason {
        let event_kind = match update.status {
            Some("revoked") => "revoke",
            Some("quarantined") => "quarantine",
            Some("rejected") => "reject_transfer",
            Some("verified") => "approve_transfer",
            _ => "override",
        };
        let details = json!({
            "status": update.status,
            "borrowed_status": update.borrowed_status,
            "verification_state": update.verification_state,
            "trust_state": update.trust_state,
            "imported_by_agent_scope": update.imported_by_agent_scope,
            "can_promote_after_verification": update.can_promote_after_verification,
        });
        record_scope_override_event(
            client,
            workspace_id,
            "import_packet",
            update.import_packet_id,
            actor_agent_id,
            event_kind,
            reason,
            &details,
        )
        .await?;
    }
    match returned_status.as_str() {
        "quarantined" => {
            let quarantine_source_event_ids =
                json!([format!("import_packet:{}", update.import_packet_id)]);
            let quarantine_evidence_span = json!({
                "kind": "import_packet_quarantine",
                "import_packet_id": update.import_packet_id,
                "status": returned_status,
                "borrowed_status": update.borrowed_status,
                "verification_state": update.verification_state,
                "trust_state": update.trust_state,
                "reason": update.reason,
                "source_evidence_span": returned_evidence_span,
            });
            let quarantine_evidence = json!({
                "summary": update.summary,
                "reason": update.reason,
                "packet_source_event_ids": returned_source_event_ids,
                "packet_artifact_refs": returned_artifact_refs,
                "packet_message_refs": returned_message_refs,
            });
            let _ = create_quarantine_item(
                client,
                &get_workspace_by_id(client, workspace_id).await?.code,
                &QuarantineItemInsert {
                    project_code: Some(target_project_code.as_str()),
                    namespace_code: None,
                    entity_kind: "import_packet",
                    entity_id: Some(update.import_packet_id),
                    quarantine_reason: update.reason.unwrap_or("import packet quarantined"),
                    quarantine_state: Some("active"),
                    evidence: &quarantine_evidence,
                    source_kind: Some("import_packet_override"),
                    source_event_ids: Some(&quarantine_source_event_ids),
                    artifact_refs: Some(&returned_artifact_refs),
                    message_refs: Some(&returned_message_refs),
                    evidence_span: Some(&quarantine_evidence_span),
                    derivation_kind: Some("operator_write"),
                    schema_version: Some("quarantine-item-envelope-v1"),
                    quarantined_at_epoch_ms: Some(current_epoch_ms()?),
                    released_at_epoch_ms: None,
                },
            )
            .await?;
        }
        "verified" | "revoked" | "rejected" => {
            let next_state = if returned_status == "verified" {
                "released"
            } else {
                "rejected"
            };
            let _ = set_quarantine_items_state_for_entity(
                client,
                workspace_id,
                "import_packet",
                update.import_packet_id,
                next_state,
                Some(current_epoch_ms()?),
            )
            .await?;
        }
        _ => {}
    }
    Ok(ImportPacketRecord {
        import_packet_id: row.get(0),
        source_project_code: row.get(1),
        target_project_code: row.get(2),
        transfer_policy_code: row.get(3),
        requested_by_agent_code: row.get(4),
        source_kind: row.get(5),
        source_event_ids: row.get(6),
        artifact_refs: row.get(7),
        message_refs: row.get(8),
        evidence_span: row.get(9),
        derivation_kind: row.get(10),
        schema_version: row.get(11),
        status: row.get(12),
        summary: row.get(13),
        allowed_by_project_link: row.get(14),
        reason: row.get(15),
        imported_by_agent_scope: row.get(16),
        trust_state: row.get(17),
        verification_state: row.get(18),
        borrowed_status: row.get(19),
        can_promote_after_verification: row.get(20),
        created_at: row.get(21),
    })
}

fn import_packet_quarantine_candidate_evidence_present(
    candidate: &ImportPacketQuarantineCandidate,
) -> bool {
    candidate.derivation_kind == "operator_write"
        || !candidate
            .source_event_ids
            .as_array()
            .unwrap_or(&vec![])
            .is_empty()
        || !candidate
            .artifact_refs
            .as_array()
            .unwrap_or(&vec![])
            .is_empty()
        || !candidate
            .message_refs
            .as_array()
            .unwrap_or(&vec![])
            .is_empty()
        || candidate
            .evidence_span
            .as_object()
            .is_some_and(|span| !span.is_empty())
}

async fn list_active_import_packet_quarantine_candidates(
    client: &Client,
    limit: Option<usize>,
) -> Result<Vec<ImportPacketQuarantineCandidate>> {
    let limit_i64 = i64::try_from(limit.unwrap_or(64)).unwrap_or(64);
    let rows = client
        .query(
            r#"
            SELECT
                qi.quarantine_item_id,
                qi.workspace_id,
                w.code,
                ip.import_packet_id,
                ip.source_project_id,
                ip.target_project_id,
                source_project.code,
                target_project.code,
                tp.code,
                COALESCE(tp.allow_import, TRUE),
                COALESCE(tp.allow_verified_writeback, TRUE),
                COALESCE(tp.requires_human_approval, FALSE),
                ip.status,
                ip.allowed_by_project_link,
                ip.imported_by_agent_scope,
                ip.evidence_span,
                ip.derivation_kind,
                ip.source_event_ids,
                ip.artifact_refs,
                ip.message_refs,
                qi.quarantine_reason
            FROM ami.quarantine_items qi
            INNER JOIN ami.workspaces w ON w.workspace_id = qi.workspace_id
            INNER JOIN ami.import_packets ip
                ON ip.import_packet_id = qi.entity_id
            INNER JOIN ami.projects source_project
                ON source_project.project_id = ip.source_project_id
            INNER JOIN ami.projects target_project
                ON target_project.project_id = ip.target_project_id
            LEFT JOIN ami.transfer_policies tp
                ON tp.transfer_policy_id = ip.transfer_policy_id
            WHERE qi.entity_kind = 'import_packet'
              AND qi.quarantine_state = 'active'
            ORDER BY qi.quarantined_at_epoch_ms ASC NULLS LAST, qi.created_at ASC
            LIMIT $1
            "#,
            &[&limit_i64],
        )
        .await
        .context("failed to list active import packet quarantine candidates")?;
    Ok(rows
        .into_iter()
        .map(|row| ImportPacketQuarantineCandidate {
            quarantine_item_id: row.get(0),
            workspace_id: row.get(1),
            workspace_code: row.get(2),
            import_packet_id: row.get(3),
            source_project_id: row.get(4),
            target_project_id: row.get(5),
            source_project_code: row.get(6),
            target_project_code: row.get(7),
            transfer_policy_allows_import: row.get(9),
            transfer_policy_allow_verified_writeback: row.get(10),
            transfer_policy_requires_human_approval: row.get(11),
            status: row.get(12),
            allowed_by_project_link: row.get(13),
            imported_by_agent_scope: row.get(14),
            evidence_span: row.get(15),
            derivation_kind: row.get(16),
            source_event_ids: row.get(17),
            artifact_refs: row.get(18),
            message_refs: row.get(19),
            quarantine_reason: row.get(20),
        })
        .collect())
}

pub async fn reconcile_import_packet_quarantines(
    client: &Client,
    apply: bool,
    limit: Option<usize>,
) -> Result<ImportPacketQuarantineResolutionSummary> {
    let candidates = list_active_import_packet_quarantine_candidates(client, limit).await?;
    let mut summary = ImportPacketQuarantineResolutionSummary {
        apply,
        scanned: candidates.len(),
        released: 0,
        rejected: 0,
        held: 0,
        decisions: Vec::with_capacity(candidates.len()),
    };

    for candidate in candidates {
        let relation = find_project_link_context(
            client,
            candidate.source_project_id,
            candidate.target_project_id,
        )
        .await?;
        let relation_present = relation.is_some();
        let relation_requires_approval = relation
            .as_ref()
            .map(|(_, _, requires_approval, _, _)| *requires_approval)
            .unwrap_or(false);
        let relation_transfer_policy_id = relation
            .as_ref()
            .and_then(|(_, _, _, transfer_policy_id, _)| *transfer_policy_id);
        let transfer_policy = load_import_packet_transfer_policy_for_writeback(
            client,
            None,
            relation_transfer_policy_id,
        )
        .await?;
        let policy_allows_verified_writeback = candidate.transfer_policy_allow_verified_writeback
            && transfer_policy
                .as_ref()
                .is_none_or(|policy| policy.allow_verified_writeback);
        let policy_requires_human_approval = candidate.transfer_policy_requires_human_approval
            || transfer_policy
                .as_ref()
                .is_some_and(|policy| policy.requires_human_approval);
        let evidence_present = import_packet_quarantine_candidate_evidence_present(&candidate);
        let poisoned = import_packet_marks_poisoned(&candidate.evidence_span);
        let same_project = candidate.source_project_id == candidate.target_project_id;
        let import_access_granted = ensure_cross_project_policy_access(
            client,
            candidate.workspace_id,
            candidate.source_project_id,
            "fact",
            &["cross_project_linked", "imported", "org_global"],
            AccessPolicyAction::Import,
            &format!(
                "reconcile import packet {} import access",
                candidate.import_packet_id
            ),
        )
        .await
        .is_ok();
        let promote_access_granted = ensure_cross_project_policy_access(
            client,
            candidate.workspace_id,
            candidate.source_project_id,
            "fact",
            &["cross_project_linked", "imported", "org_global"],
            AccessPolicyAction::Promote,
            &format!(
                "reconcile import packet {} promote access",
                candidate.import_packet_id
            ),
        )
        .await
        .is_ok();
        let approve_transfer_granted = ensure_cross_project_policy_access(
            client,
            candidate.workspace_id,
            candidate.source_project_id,
            "fact",
            &["cross_project_linked", "imported", "org_global"],
            AccessPolicyAction::ApproveTransfer,
            &format!(
                "reconcile import packet {} approve access",
                candidate.import_packet_id
            ),
        )
        .await
        .is_ok();

        let (decision, reason) = if candidate.status == "verified" {
            (
                "release".to_string(),
                "autonomous quarantine repair passed: packet already verified, releasing stale quarantine"
                    .to_string(),
            )
        } else if candidate.status == "rejected" {
            (
                "reject".to_string(),
                "autonomous quarantine repair passed: packet already rejected, finalizing stale quarantine"
                    .to_string(),
            )
        } else {
            let mut blockers = Vec::new();
            if candidate.status != "quarantined" {
                blockers.push(format!("unexpected_status={}", candidate.status));
            }
            if !candidate.allowed_by_project_link {
                blockers.push("packet_allowed_by_project_link=false".to_string());
            }
            if !relation_present {
                blockers.push("project_link_missing".to_string());
            }
            if relation_requires_approval {
                blockers.push("relation_requires_approval".to_string());
            }
            if !candidate.transfer_policy_allows_import {
                blockers.push("transfer_policy_disallows_import".to_string());
            }
            if !policy_allows_verified_writeback {
                blockers.push("transfer_policy_disallows_verified_writeback".to_string());
            }
            if policy_requires_human_approval {
                blockers.push("transfer_policy_requires_human_approval".to_string());
            }
            if !import_access_granted {
                blockers.push("access_policy_import_denied".to_string());
            }
            if !promote_access_granted {
                blockers.push("access_policy_promote_denied".to_string());
            }
            if !approve_transfer_granted {
                blockers.push("access_policy_approve_transfer_denied".to_string());
            }
            if !evidence_present {
                blockers.push("evidence_missing".to_string());
            }
            if poisoned {
                blockers.push("poisoned_evidence".to_string());
            }
            if same_project {
                blockers.push("same_project_conflict".to_string());
            }

            if blockers.is_empty() {
                (
                    "release".to_string(),
                    "autonomous quarantine review passed: policy and evidence checks satisfied"
                        .to_string(),
                )
            } else if candidate.status != "quarantined" {
                (
                    "hold".to_string(),
                    format!(
                        "autonomous quarantine review deferred: {}",
                        blockers.join(", ")
                    ),
                )
            } else {
                (
                    "reject".to_string(),
                    format!(
                        "autonomous quarantine review failed: {}",
                        blockers.join(", ")
                    ),
                )
            }
        };

        let mut action_applied = false;
        if apply {
            match decision.as_str() {
                "release" => {
                    if candidate.status == "verified" {
                        let released_at_epoch_ms = current_epoch_ms()?;
                        let _ = set_quarantine_items_state_for_entity(
                            client,
                            candidate.workspace_id,
                            "import_packet",
                            candidate.import_packet_id,
                            "released",
                            Some(released_at_epoch_ms),
                        )
                        .await?;
                    } else {
                        let actor_agent_code = ensure_autonomous_quarantine_governor_agent(
                            client,
                            &candidate.workspace_code,
                        )
                        .await?;
                        let _ = update_import_packet(
                            client,
                            ImportPacketUpdate {
                                import_packet_id: candidate.import_packet_id,
                                status: Some("verified"),
                                summary: Some(
                                    "Amai autonomous quarantine resolver released import packet",
                                ),
                                reason: Some(reason.as_str()),
                                imported_by_agent_scope: Some(
                                    candidate.imported_by_agent_scope.as_str(),
                                ),
                                trust_state: Some("verified"),
                                verification_state: Some("verified"),
                                borrowed_status: Some("verified_local_copy"),
                                can_promote_after_verification: Some(true),
                                actor_agent_code: Some(actor_agent_code.as_str()),
                            },
                        )
                        .await?;
                    }
                    action_applied = true;
                }
                "reject" => {
                    if candidate.status == "rejected" {
                        let _ = set_quarantine_items_state_for_entity(
                            client,
                            candidate.workspace_id,
                            "import_packet",
                            candidate.import_packet_id,
                            "rejected",
                            None,
                        )
                        .await?;
                    } else {
                        let actor_agent_code = ensure_autonomous_quarantine_governor_agent(
                            client,
                            &candidate.workspace_code,
                        )
                        .await?;
                        let _ = update_import_packet(
                            client,
                            ImportPacketUpdate {
                                import_packet_id: candidate.import_packet_id,
                                status: Some("rejected"),
                                summary: Some(
                                    "Amai autonomous quarantine resolver rejected import packet",
                                ),
                                reason: Some(reason.as_str()),
                                imported_by_agent_scope: None,
                                trust_state: Some("disputed"),
                                verification_state: Some("rejected"),
                                borrowed_status: Some("rejected"),
                                can_promote_after_verification: Some(false),
                                actor_agent_code: Some(actor_agent_code.as_str()),
                            },
                        )
                        .await?;
                    }
                    action_applied = true;
                }
                _ => {}
            }
        }

        match decision.as_str() {
            "release" => summary.released += 1,
            "reject" => summary.rejected += 1,
            _ => summary.held += 1,
        }
        summary
            .decisions
            .push(ImportPacketQuarantineDecisionRecord {
                quarantine_item_id: candidate.quarantine_item_id,
                import_packet_id: candidate.import_packet_id,
                workspace_code: candidate.workspace_code,
                source_project_code: candidate.source_project_code,
                target_project_code: candidate.target_project_code,
                decision,
                action_applied,
                quarantine_reason: candidate.quarantine_reason,
                reason,
            });
    }

    Ok(summary)
}

#[derive(Debug, Clone)]
struct ImportPacketPolicyScopeFilter {
    source_project_code: String,
    target_project_code: String,
    project_link_present: bool,
    transfer_policy_code: Option<String>,
    transfer_policy_allows_import: bool,
    access_policy_import_granted: bool,
    approval_required: bool,
    verified_import_blocked_until_approval: bool,
    scope_allowed: bool,
}

#[derive(Debug, Clone)]
struct ImportPacketVerificationConflictCheck {
    evidence_present: bool,
    poisoned_detected: bool,
    same_project_conflict: bool,
    write_allowed: bool,
}

#[derive(Debug, Clone)]
struct ImportPacketQuarantineCandidate {
    quarantine_item_id: Uuid,
    workspace_id: Uuid,
    workspace_code: String,
    import_packet_id: Uuid,
    source_project_id: Uuid,
    target_project_id: Uuid,
    source_project_code: String,
    target_project_code: String,
    transfer_policy_allows_import: bool,
    transfer_policy_allow_verified_writeback: bool,
    transfer_policy_requires_human_approval: bool,
    status: String,
    allowed_by_project_link: bool,
    imported_by_agent_scope: String,
    evidence_span: Value,
    derivation_kind: String,
    source_event_ids: Value,
    artifact_refs: Value,
    message_refs: Value,
    quarantine_reason: String,
}

fn import_packet_marks_poisoned(evidence_span: &Value) -> bool {
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

fn validate_import_packet_policy_scope_filter(
    filter: &ImportPacketPolicyScopeFilter,
) -> Result<()> {
    if !filter.project_link_present {
        return Err(anyhow!(
            "cross-project import is blocked without active project_link: {} -> {}",
            filter.source_project_code,
            filter.target_project_code
        ));
    }
    if !filter.transfer_policy_allows_import {
        return Err(anyhow!(
            "transfer policy {:?} blocks import for {} -> {}",
            filter.transfer_policy_code,
            filter.source_project_code,
            filter.target_project_code
        ));
    }
    if !filter.access_policy_import_granted {
        return Err(anyhow!(
            "import packet violates access policy for {} -> {}",
            filter.source_project_code,
            filter.target_project_code
        ));
    }
    if filter.verified_import_blocked_until_approval {
        return Err(anyhow!(
            "project_link {} -> {} requires approval before verified import",
            filter.source_project_code,
            filter.target_project_code
        ));
    }
    if !filter.scope_allowed {
        return Err(anyhow!(
            "import packet failed policy/scope filter for {} -> {}",
            filter.source_project_code,
            filter.target_project_code
        ));
    }
    Ok(())
}

fn validate_import_packet_verification_conflict_check(
    check: &ImportPacketVerificationConflictCheck,
) -> Result<()> {
    if check.poisoned_detected {
        return Err(anyhow!(
            "import packet is flagged poisoned by evidence span and cannot be written"
        ));
    }
    if check.same_project_conflict {
        return Err(anyhow!(
            "import packet requires distinct source and target projects"
        ));
    }
    if !check.evidence_present {
        return Err(anyhow!(
            "import packet must carry evidence unless written as operator hot-path override"
        ));
    }
    if !check.write_allowed {
        return Err(anyhow!(
            "import packet failed verification/conflict check before truth write"
        ));
    }
    Ok(())
}

fn augment_import_packet_evidence_span_with_stage2_preflight(
    evidence_span: &Value,
    policy_filter: &ImportPacketPolicyScopeFilter,
    verification_check: &ImportPacketVerificationConflictCheck,
) -> Value {
    let mut object = match evidence_span {
        Value::Object(map) => map.clone(),
        _ => {
            let mut fallback = serde_json::Map::new();
            fallback.insert("user_evidence_span".to_string(), evidence_span.clone());
            fallback
        }
    };
    object.insert(
        "stage2_runtime".to_string(),
        json!({
            "policy_and_scope_filter": {
                "source_project_code": policy_filter.source_project_code,
                "target_project_code": policy_filter.target_project_code,
                "project_link_present": policy_filter.project_link_present,
                "transfer_policy_code": policy_filter.transfer_policy_code,
                "transfer_policy_allows_import": policy_filter.transfer_policy_allows_import,
                "access_policy_import_granted": policy_filter.access_policy_import_granted,
                "approval_required": policy_filter.approval_required,
                "verified_import_blocked_until_approval": policy_filter.verified_import_blocked_until_approval,
                "scope_allowed": policy_filter.scope_allowed,
            },
            "verification_conflict_check": {
                "evidence_present": verification_check.evidence_present,
                "poisoned_detected": verification_check.poisoned_detected,
                "same_project_conflict": verification_check.same_project_conflict,
                "write_allowed": verification_check.write_allowed,
            }
        }),
    );
    Value::Object(object)
}
