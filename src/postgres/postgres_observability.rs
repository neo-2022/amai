use super::*;
use anyhow::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ObservabilityInsertErrorPhase {
    BeforeWrite,
    OutcomeUnknownAfterWrite,
}

#[derive(Debug)]
pub(crate) struct ObservabilityInsertError {
    pub(crate) phase: ObservabilityInsertErrorPhase,
    pub(crate) snapshot_kind: String,
    pub(crate) event_key: String,
    pub(crate) sqlstate_code: Option<String>,
    pub(crate) error: Error,
}

impl ObservabilityInsertError {
    fn before_write(snapshot_kind: &str, event_key: &str, error: Error) -> Self {
        Self {
            phase: ObservabilityInsertErrorPhase::BeforeWrite,
            snapshot_kind: snapshot_kind.to_string(),
            event_key: event_key.to_string(),
            sqlstate_code: extract_sqlstate_code(&error),
            error,
        }
    }

    fn before_write_with_sqlstate(
        snapshot_kind: &str,
        event_key: &str,
        sqlstate_code: Option<String>,
        error: Error,
    ) -> Self {
        Self {
            phase: ObservabilityInsertErrorPhase::BeforeWrite,
            snapshot_kind: snapshot_kind.to_string(),
            event_key: event_key.to_string(),
            sqlstate_code,
            error,
        }
    }

    fn outcome_unknown_after_write(
        snapshot_kind: &str,
        event_key: &str,
        sqlstate_code: Option<String>,
        error: Error,
    ) -> Self {
        Self {
            phase: ObservabilityInsertErrorPhase::OutcomeUnknownAfterWrite,
            snapshot_kind: snapshot_kind.to_string(),
            event_key: event_key.to_string(),
            sqlstate_code,
            error,
        }
    }
}

pub async fn insert_observability_snapshot(
    client: &Client,
    snapshot_kind: &str,
    payload: &Value,
) -> Result<Uuid> {
    insert_observability_snapshot_detailed(client, snapshot_kind, payload)
        .await
        .map_err(|error| error.error)
}

pub(crate) async fn lookup_observability_snapshot_id_for_payload(
    client: &Client,
    snapshot_kind: &str,
    payload: &Value,
) -> Result<Option<Uuid>> {
    let (_, meta) = prepare_observability_payload(snapshot_kind, payload)?;
    lookup_observability_snapshot_id_by_event_key(client, snapshot_kind, &meta.event_key).await
}

pub(crate) async fn insert_observability_snapshot_detailed(
    client: &Client,
    snapshot_kind: &str,
    payload: &Value,
) -> std::result::Result<Uuid, ObservabilityInsertError> {
    let prepare_started = Instant::now();
    let (stored_payload, meta) = prepare_observability_payload(snapshot_kind, payload)
        .map_err(|error| ObservabilityInsertError::before_write(snapshot_kind, "prepare_failed", error))?;
    observability_profile_log(
        "insert_observability_snapshot.prepare_payload",
        prepare_started.elapsed().as_millis(),
        &format!("snapshot_kind={snapshot_kind}"),
    );
    #[cfg(test)]
    if let Some(forced_error) =
        forced_observability_insert_error_for_tests(snapshot_kind, &meta.event_key, false)
    {
        return Err(forced_error);
    }
    let insert_started = Instant::now();
    let row = client
        .query_opt(
            r#"
            INSERT INTO ami.observability_snapshots(
                snapshot_kind,
                payload,
                event_key,
                source_event_id,
                source_kind,
                source_class,
                scope_project_code,
                scope_namespace_code,
                captured_at_epoch_ms,
                payload_sha256,
                last_seen_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, now())
            ON CONFLICT (snapshot_kind, event_key) DO UPDATE
            SET payload = CASE
                    WHEN $1 = 'working_state_restore'
                     AND ami.observability_snapshots.source_event_id IS NOT NULL
                     AND EXCLUDED.source_event_id IS NOT NULL
                     AND ami.observability_snapshots.source_event_id = EXCLUDED.source_event_id
                     AND EXCLUDED.captured_at_epoch_ms IS NOT NULL
                     AND ami.observability_snapshots.captured_at_epoch_ms IS NOT NULL
                     AND EXCLUDED.captured_at_epoch_ms <= ami.observability_snapshots.captured_at_epoch_ms
                    THEN ami.observability_snapshots.payload
                    ELSE EXCLUDED.payload
                END,
                source_event_id = CASE
                    WHEN $1 = 'working_state_restore'
                     AND ami.observability_snapshots.source_event_id IS NOT NULL
                     AND EXCLUDED.source_event_id IS NOT NULL
                     AND ami.observability_snapshots.source_event_id = EXCLUDED.source_event_id
                     AND EXCLUDED.captured_at_epoch_ms IS NOT NULL
                     AND ami.observability_snapshots.captured_at_epoch_ms IS NOT NULL
                     AND EXCLUDED.captured_at_epoch_ms <= ami.observability_snapshots.captured_at_epoch_ms
                    THEN ami.observability_snapshots.source_event_id
                    ELSE EXCLUDED.source_event_id
                END,
                source_kind = CASE
                    WHEN $1 = 'working_state_restore'
                     AND ami.observability_snapshots.source_event_id IS NOT NULL
                     AND EXCLUDED.source_event_id IS NOT NULL
                     AND ami.observability_snapshots.source_event_id = EXCLUDED.source_event_id
                     AND EXCLUDED.captured_at_epoch_ms IS NOT NULL
                     AND ami.observability_snapshots.captured_at_epoch_ms IS NOT NULL
                     AND EXCLUDED.captured_at_epoch_ms <= ami.observability_snapshots.captured_at_epoch_ms
                    THEN ami.observability_snapshots.source_kind
                    ELSE EXCLUDED.source_kind
                END,
                source_class = CASE
                    WHEN $1 = 'working_state_restore'
                     AND ami.observability_snapshots.source_event_id IS NOT NULL
                     AND EXCLUDED.source_event_id IS NOT NULL
                     AND ami.observability_snapshots.source_event_id = EXCLUDED.source_event_id
                     AND EXCLUDED.captured_at_epoch_ms IS NOT NULL
                     AND ami.observability_snapshots.captured_at_epoch_ms IS NOT NULL
                     AND EXCLUDED.captured_at_epoch_ms <= ami.observability_snapshots.captured_at_epoch_ms
                    THEN ami.observability_snapshots.source_class
                    ELSE EXCLUDED.source_class
                END,
                scope_project_code = CASE
                    WHEN $1 = 'working_state_restore'
                     AND ami.observability_snapshots.source_event_id IS NOT NULL
                     AND EXCLUDED.source_event_id IS NOT NULL
                     AND ami.observability_snapshots.source_event_id = EXCLUDED.source_event_id
                     AND EXCLUDED.captured_at_epoch_ms IS NOT NULL
                     AND ami.observability_snapshots.captured_at_epoch_ms IS NOT NULL
                     AND EXCLUDED.captured_at_epoch_ms <= ami.observability_snapshots.captured_at_epoch_ms
                    THEN ami.observability_snapshots.scope_project_code
                    ELSE EXCLUDED.scope_project_code
                END,
                scope_namespace_code = CASE
                    WHEN $1 = 'working_state_restore'
                     AND ami.observability_snapshots.source_event_id IS NOT NULL
                     AND EXCLUDED.source_event_id IS NOT NULL
                     AND ami.observability_snapshots.source_event_id = EXCLUDED.source_event_id
                     AND EXCLUDED.captured_at_epoch_ms IS NOT NULL
                     AND ami.observability_snapshots.captured_at_epoch_ms IS NOT NULL
                     AND EXCLUDED.captured_at_epoch_ms <= ami.observability_snapshots.captured_at_epoch_ms
                    THEN ami.observability_snapshots.scope_namespace_code
                    ELSE EXCLUDED.scope_namespace_code
                END,
                captured_at_epoch_ms = CASE
                    WHEN $1 = 'working_state_restore'
                     AND ami.observability_snapshots.source_event_id IS NOT NULL
                     AND EXCLUDED.source_event_id IS NOT NULL
                     AND ami.observability_snapshots.source_event_id = EXCLUDED.source_event_id
                     AND EXCLUDED.captured_at_epoch_ms IS NOT NULL
                     AND ami.observability_snapshots.captured_at_epoch_ms IS NOT NULL
                     AND EXCLUDED.captured_at_epoch_ms <= ami.observability_snapshots.captured_at_epoch_ms
                    THEN ami.observability_snapshots.captured_at_epoch_ms
                    ELSE EXCLUDED.captured_at_epoch_ms
                END,
                payload_sha256 = CASE
                    WHEN $1 = 'working_state_restore'
                     AND ami.observability_snapshots.source_event_id IS NOT NULL
                     AND EXCLUDED.source_event_id IS NOT NULL
                     AND ami.observability_snapshots.source_event_id = EXCLUDED.source_event_id
                     AND EXCLUDED.captured_at_epoch_ms IS NOT NULL
                     AND ami.observability_snapshots.captured_at_epoch_ms IS NOT NULL
                     AND EXCLUDED.captured_at_epoch_ms <= ami.observability_snapshots.captured_at_epoch_ms
                    THEN ami.observability_snapshots.payload_sha256
                    ELSE EXCLUDED.payload_sha256
                END,
                replay_count = ami.observability_snapshots.replay_count + 1,
                last_seen_at = now()
            WHERE ami.observability_snapshots.payload_sha256 = EXCLUDED.payload_sha256
               OR (
                    ami.observability_snapshots.source_event_id IS NOT NULL
                    AND EXCLUDED.source_event_id IS NOT NULL
                    AND ami.observability_snapshots.source_event_id = EXCLUDED.source_event_id
                    AND EXCLUDED.captured_at_epoch_ms IS NOT NULL
                    AND ami.observability_snapshots.captured_at_epoch_ms IS NOT NULL
                    AND EXCLUDED.captured_at_epoch_ms <= ami.observability_snapshots.captured_at_epoch_ms
               )
               OR (
                    $1 = 'working_state_restore'
                    AND ami.observability_snapshots.source_event_id IS NOT NULL
                    AND EXCLUDED.source_event_id IS NOT NULL
                    AND ami.observability_snapshots.source_event_id = EXCLUDED.source_event_id
                    AND (
                        EXCLUDED.captured_at_epoch_ms IS NULL
                        OR ami.observability_snapshots.captured_at_epoch_ms IS NULL
                        OR EXCLUDED.captured_at_epoch_ms >= ami.observability_snapshots.captured_at_epoch_ms
                    )
               )
            RETURNING snapshot_id
            "#,
            &[
                &snapshot_kind,
                &stored_payload,
                &meta.event_key,
                &meta.source_event_id,
                &meta.source_kind,
                &meta.source_class,
                &meta.scope_project_code,
                &meta.scope_namespace_code,
                &meta.captured_at_epoch_ms,
                &meta.payload_sha256,
            ],
        )
        .await
        .context("failed to insert observability snapshot")
        .map_err(|error| {
            ObservabilityInsertError::before_write(snapshot_kind, &meta.event_key, error)
        })?;
    observability_profile_log(
        "insert_observability_snapshot.sql_insert",
        insert_started.elapsed().as_millis(),
        &format!("snapshot_kind={snapshot_kind} conflict={}", row.is_none()),
    );
    if let Some(row) = row {
        #[cfg(test)]
        if let Some(forced_error) =
            forced_observability_insert_error_for_tests(snapshot_kind, &meta.event_key, true)
        {
            return Err(forced_error);
        }
        return Ok(row.get(0));
    }

    let inspect_started = Instant::now();
    let existing = client
        .query_opt(
            r#"
            SELECT snapshot_id, source_event_id, captured_at_epoch_ms
            FROM ami.observability_snapshots
            WHERE snapshot_kind = $1
              AND event_key = $2
            "#,
            &[&snapshot_kind, &meta.event_key],
        )
        .await
        .context("failed to inspect conflicting observability snapshot")
        .map_err(|error| {
            ObservabilityInsertError::before_write(snapshot_kind, &meta.event_key, error)
        })?
        .ok_or_else(|| {
            ObservabilityInsertError::before_write(snapshot_kind, &meta.event_key, anyhow!(
                "observability snapshot conflict vanished unexpectedly for {} :: {}",
                snapshot_kind,
                meta.event_key
            ))
        })?;
    observability_profile_log(
        "insert_observability_snapshot.inspect_conflict",
        inspect_started.elapsed().as_millis(),
        &format!("snapshot_kind={snapshot_kind}"),
    );
    let existing_snapshot_id: Uuid = existing.get(0);
    let existing_source_event_id: Option<String> = existing.get(1);
    let existing_captured_at_epoch_ms: Option<i64> = existing.get(2);

    if meta.source_event_id.is_some()
        && meta.source_event_id == existing_source_event_id
        && meta
            .captured_at_epoch_ms
            .zip(existing_captured_at_epoch_ms)
            .is_some_and(|(incoming, existing)| incoming > existing)
    {
        return Err(ObservabilityInsertError::before_write(
            snapshot_kind,
            &meta.event_key,
            observability_conflict_error(
                snapshot_kind,
                &meta,
                existing_snapshot_id,
                existing_source_event_id.as_deref(),
                existing_captured_at_epoch_ms,
            ),
        ));
    }

    Err(ObservabilityInsertError::before_write(
        snapshot_kind,
        &meta.event_key,
        observability_conflict_error(
            snapshot_kind,
            &meta,
            existing_snapshot_id,
            existing_source_event_id.as_deref(),
            existing_captured_at_epoch_ms,
        ),
    ))
}

async fn lookup_observability_snapshot_id_by_event_key(
    client: &Client,
    snapshot_kind: &str,
    event_key: &str,
) -> Result<Option<Uuid>> {
    client
        .query_opt(
            r#"
            SELECT snapshot_id
            FROM ami.observability_snapshots
            WHERE snapshot_kind = $1
              AND event_key = $2
            "#,
            &[&snapshot_kind, &event_key],
        )
        .await
        .context("failed to lookup observability snapshot by event_key")
        .map(|row| row.map(|row| row.get(0)))
}

pub async fn insert_execctl_task_ledger_entry(
    client: &Client,
    entry: &ExecCtlTaskLedgerEntryInsert<'_>,
) -> Result<Uuid> {
    if entry.source_event_id.trim().is_empty() {
        return Err(anyhow!(
            "execctl task ledger source_event_id must not be empty"
        ));
    }
    if entry.agent_scope.trim().is_empty() {
        return Err(anyhow!("execctl task ledger agent_scope must not be empty"));
    }
    let inserted = client
        .query_opt(
            r#"
            INSERT INTO ami.execctl_task_ledger_entries(
                project_id,
                namespace_id,
                agent_scope,
                session_id,
                thread_id,
                source_snapshot_id,
                source_event_id,
                event_kind,
                source_kind,
                headline,
                next_step,
                summary,
                active_files,
                open_questions,
                materialized_notes,
                pending_return_queue,
                local_path,
                recorded_at_epoch_ms
            )
            VALUES (
                $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18
            )
            ON CONFLICT (source_event_id) DO NOTHING
            RETURNING ledger_entry_id
            "#,
            &[
                &entry.project_id,
                &entry.namespace_id,
                &entry.agent_scope,
                &entry.session_id,
                &entry.thread_id,
                &entry.source_snapshot_id,
                &entry.source_event_id,
                &entry.event_kind,
                &entry.source_kind,
                &entry.headline,
                &entry.next_step,
                &entry.summary,
                entry.active_files,
                entry.open_questions,
                entry.materialized_notes,
                entry.pending_return_queue,
                &entry.local_path,
                &entry.recorded_at_epoch_ms,
            ],
        )
        .await
        .context("failed to insert execctl task ledger entry")?;
    if let Some(row) = inserted {
        return Ok(row.get(0));
    }

    let existing = client
        .query_one(
            r#"
            SELECT ledger_entry_id
            FROM ami.execctl_task_ledger_entries
            WHERE source_event_id = $1
            "#,
            &[&entry.source_event_id],
        )
        .await
        .context("failed to resolve existing execctl task ledger entry")?;
    Ok(existing.get(0))
}

pub async fn list_execctl_task_ledger_entries(
    client: &Client,
    project_id: Uuid,
    namespace_id: Uuid,
    agent_scope: &str,
    limit: Option<i64>,
) -> Result<Vec<ExecCtlTaskLedgerEntryRecord>> {
    let limit = limit.unwrap_or(i64::MAX);
    let rows = client
        .query(
            r#"
            SELECT
                ledger_entry_id,
                source_snapshot_id,
                source_event_id,
                event_kind,
                source_kind,
                agent_scope,
                session_id,
                thread_id,
                headline,
                next_step,
                summary,
                active_files,
                open_questions,
                materialized_notes,
                pending_return_queue,
                local_path,
                recorded_at_epoch_ms,
                (EXTRACT(EPOCH FROM created_at) * 1000)::bigint AS created_at_epoch_ms
            FROM ami.execctl_task_ledger_entries
            WHERE project_id = $1
              AND namespace_id = $2
              AND agent_scope = $3
            ORDER BY recorded_at_epoch_ms DESC, created_at DESC
            LIMIT $4
            "#,
            &[&project_id, &namespace_id, &agent_scope, &limit],
        )
        .await
        .context("failed to list execctl task ledger entries")?;
    Ok(rows
        .into_iter()
        .map(|row| ExecCtlTaskLedgerEntryRecord {
            ledger_entry_id: row.get(0),
            source_snapshot_id: row.get(1),
            source_event_id: row.get(2),
            event_kind: row.get(3),
            source_kind: row.get(4),
            agent_scope: row.get(5),
            session_id: row.get(6),
            thread_id: row.get(7),
            headline: row.get(8),
            next_step: row.get(9),
            summary: row.get(10),
            active_files: row.get(11),
            open_questions: row.get(12),
            materialized_notes: row.get(13),
            pending_return_queue: row.get(14),
            local_path: row.get(15),
            recorded_at_epoch_ms: row.get(16),
            created_at_epoch_ms: row.get(17),
        })
        .collect())
}

pub async fn upsert_execctl_task_lease(
    client: &Client,
    lease: &ExecCtlTaskLeaseInsert<'_>,
) -> Result<Uuid> {
    if lease.agent_scope.trim().is_empty() {
        return Err(anyhow!("execctl task lease agent_scope must not be empty"));
    }
    if lease.source_event_id.trim().is_empty() {
        return Err(anyhow!(
            "execctl task lease source_event_id must not be empty"
        ));
    }
    let row = client
        .query_one(
            r#"
            INSERT INTO ami.execctl_task_leases(
                project_id,
                namespace_id,
                agent_scope,
                owner_session_id,
                owner_thread_id,
                source_snapshot_id,
                source_event_id,
                source_kind,
                lease_state,
                headline,
                next_step,
                local_path,
                acquired_at_epoch_ms,
                heartbeat_at_epoch_ms,
                expires_at_epoch_ms
            )
            VALUES (
                $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15
            )
            ON CONFLICT (project_id, namespace_id, agent_scope) DO UPDATE
            SET owner_session_id = EXCLUDED.owner_session_id,
                owner_thread_id = EXCLUDED.owner_thread_id,
                source_snapshot_id = EXCLUDED.source_snapshot_id,
                source_event_id = EXCLUDED.source_event_id,
                source_kind = EXCLUDED.source_kind,
                lease_state = EXCLUDED.lease_state,
                headline = EXCLUDED.headline,
                next_step = EXCLUDED.next_step,
                local_path = EXCLUDED.local_path,
                acquired_at_epoch_ms = EXCLUDED.acquired_at_epoch_ms,
                heartbeat_at_epoch_ms = EXCLUDED.heartbeat_at_epoch_ms,
                expires_at_epoch_ms = EXCLUDED.expires_at_epoch_ms,
                updated_at = now()
            RETURNING lease_id
            "#,
            &[
                &lease.project_id,
                &lease.namespace_id,
                &lease.agent_scope,
                &lease.owner_session_id,
                &lease.owner_thread_id,
                &lease.source_snapshot_id,
                &lease.source_event_id,
                &lease.source_kind,
                &lease.lease_state,
                &lease.headline,
                &lease.next_step,
                &lease.local_path,
                &lease.acquired_at_epoch_ms,
                &lease.heartbeat_at_epoch_ms,
                &lease.expires_at_epoch_ms,
            ],
        )
        .await
        .context("failed to upsert execctl task lease")?;
    Ok(row.get(0))
}

pub async fn get_execctl_task_lease(
    client: &Client,
    project_id: Uuid,
    namespace_id: Uuid,
    agent_scope: &str,
    min_expires_at_epoch_ms: i64,
) -> Result<Option<ExecCtlTaskLeaseRecord>> {
    let row = client
        .query_opt(
            r#"
            SELECT
                lease_id,
                source_snapshot_id,
                source_event_id,
                source_kind,
                agent_scope,
                owner_session_id,
                owner_thread_id,
                lease_state,
                headline,
                next_step,
                local_path,
                acquired_at_epoch_ms,
                heartbeat_at_epoch_ms,
                expires_at_epoch_ms,
                (EXTRACT(EPOCH FROM created_at) * 1000)::bigint AS created_at_epoch_ms,
                (EXTRACT(EPOCH FROM updated_at) * 1000)::bigint AS updated_at_epoch_ms
            FROM ami.execctl_task_leases
            WHERE project_id = $1
              AND namespace_id = $2
              AND agent_scope = $3
              AND expires_at_epoch_ms >= $4
            ORDER BY updated_at DESC
            LIMIT 1
            "#,
            &[
                &project_id,
                &namespace_id,
                &agent_scope,
                &min_expires_at_epoch_ms,
            ],
        )
        .await
        .context("failed to fetch execctl task lease")?;
    Ok(row.map(|row| ExecCtlTaskLeaseRecord {
        lease_id: row.get(0),
        source_snapshot_id: row.get(1),
        source_event_id: row.get(2),
        source_kind: row.get(3),
        agent_scope: row.get(4),
        owner_session_id: row.get(5),
        owner_thread_id: row.get(6),
        lease_state: row.get(7),
        headline: row.get(8),
        next_step: row.get(9),
        local_path: row.get(10),
        acquired_at_epoch_ms: row.get(11),
        heartbeat_at_epoch_ms: row.get(12),
        expires_at_epoch_ms: row.get(13),
        created_at_epoch_ms: row.get(14),
        updated_at_epoch_ms: row.get(15),
    }))
}

pub async fn delete_observability_snapshots_by_scope(
    client: &Client,
    snapshot_kind: &str,
    payload_root: &str,
    project_code: &str,
    namespace_code: &str,
) -> Result<u64> {
    let sql = format!(
        r#"
        DELETE FROM ami.observability_snapshots
        WHERE snapshot_kind = $1
          AND payload->'{payload_root}'->'project'->>'code' = $2
          AND payload->'{payload_root}'->'namespace'->>'code' = $3
        "#
    );
    client
        .execute(&sql, &[&snapshot_kind, &project_code, &namespace_code])
        .await
        .context("failed to delete scoped observability snapshots")
}

pub async fn latest_observability_snapshot(
    client: &Client,
    snapshot_kind: &str,
) -> Result<Option<Value>> {
    let row = client
        .query_opt(
            r#"
            SELECT payload
            FROM ami.observability_snapshots
            WHERE snapshot_kind = $1
            ORDER BY created_at DESC
            LIMIT 1
            "#,
            &[&snapshot_kind],
        )
        .await?;
    Ok(row.map(|row| row.get(0)))
}

pub async fn latest_observability_snapshot_for_project(
    client: &Client,
    snapshot_kind: &str,
    payload_root: &str,
    project_code: &str,
) -> Result<Option<Value>> {
    let scoped_row = client
        .query_opt(
            r#"
            SELECT payload
            FROM ami.observability_snapshots
            WHERE snapshot_kind = $1
              AND scope_project_code = $2
            ORDER BY created_at DESC
            LIMIT 1
            "#,
            &[&snapshot_kind, &project_code],
        )
        .await?;
    if let Some(row) = scoped_row {
        return Ok(Some(row.get(0)));
    }

    let legacy_row = client
        .query_opt(
            &format!(
                r#"
                SELECT payload
                FROM ami.observability_snapshots
                WHERE snapshot_kind = $1
                  AND payload->'{payload_root}'->'project'->>'code' = $2
                ORDER BY created_at DESC
                LIMIT 1
                "#
            ),
            &[&snapshot_kind, &project_code],
        )
        .await?;
    Ok(legacy_row.map(|row| row.get(0)))
}

pub async fn list_observability_snapshots_by_kinds(
    client: &Client,
    kinds: &[&str],
    limit: Option<i64>,
) -> Result<Vec<ObservabilitySnapshotRecord>> {
    if kinds.is_empty() {
        return Ok(Vec::new());
    }
    let limit = limit.unwrap_or(i64::MAX);
    let rows = client
        .query(
            r#"
            SELECT
                snapshot_id,
                snapshot_kind,
                payload,
                (EXTRACT(EPOCH FROM created_at) * 1000)::bigint AS created_at_epoch_ms
            FROM ami.observability_snapshots
            WHERE snapshot_kind = ANY($1::text[])
            ORDER BY created_at DESC
            LIMIT $2
            "#,
            &[&kinds, &limit],
        )
        .await
        .context("failed to list observability snapshots by kinds")?;
    Ok(rows
        .into_iter()
        .map(|row| ObservabilitySnapshotRecord {
            snapshot_id: row.get(0),
            snapshot_kind: row.get(1),
            payload: row.get(2),
            created_at_epoch_ms: row.get(3),
        })
        .collect())
}

pub async fn list_observability_snapshots_by_kind_for_scope(
    client: &Client,
    snapshot_kind: &str,
    payload_root: &str,
    project_code: &str,
    namespace_code: &str,
    limit: Option<i64>,
) -> Result<Vec<ObservabilitySnapshotRecord>> {
    let limit = limit.unwrap_or(i64::MAX);
    let scoped_rows = query_observability_snapshots_by_kind_for_scope_index_only(
        client,
        snapshot_kind,
        project_code,
        namespace_code,
        limit,
    )
    .await
    .with_context(|| {
        format!(
            "failed to list scoped observability snapshots (indexed scope path) for {}::{}::{}",
            snapshot_kind, project_code, namespace_code
        )
    })?;
    if !scoped_rows.is_empty() {
        return Ok(scoped_rows);
    }
    let fallback_rows = client
        .query(
            r#"
            SELECT
                snapshot_id,
                snapshot_kind,
                payload,
                (EXTRACT(EPOCH FROM created_at) * 1000)::bigint AS created_at_epoch_ms
            FROM ami.observability_snapshots
            WHERE snapshot_kind = $1
              AND (
                    (scope_project_code = $2 AND scope_namespace_code = $3)
                    OR (
                        payload #>> ARRAY[$4, 'project', 'code'] = $2
                        AND payload #>> ARRAY[$4, 'namespace', 'code'] = $3
                    )
                  )
            ORDER BY COALESCE(
                        captured_at_epoch_ms,
                        NULLIF(payload #>> ARRAY[$4, 'captured_at_epoch_ms'], '')::bigint,
                        NULLIF(payload #>> ARRAY[$4, 'imported_at_epoch_ms'], '')::bigint,
                        CASE
                            WHEN NULLIF(payload #>> ARRAY[$4, 'created_at_epoch_s'], '') IS NULL
                                THEN NULL
                            ELSE (NULLIF(payload #>> ARRAY[$4, 'created_at_epoch_s'], '')::bigint * 1000)
                        END,
                        (EXTRACT(EPOCH FROM created_at) * 1000)::bigint
                     ) DESC,
                     created_at DESC
            LIMIT $5
            "#,
            &[
                &snapshot_kind,
                &project_code,
                &namespace_code,
                &payload_root,
                &limit,
            ],
        )
        .await
        .with_context(|| {
            format!(
                "failed to list scoped observability snapshots (payload fallback path) for {}::{}::{}",
                snapshot_kind, project_code, namespace_code
            )
        })?;
    Ok(fallback_rows
        .into_iter()
        .map(|row| ObservabilitySnapshotRecord {
            snapshot_id: row.get(0),
            snapshot_kind: row.get(1),
            payload: row.get(2),
            created_at_epoch_ms: row.get(3),
        })
        .collect())
}

pub async fn list_observability_snapshots_by_kind_for_scope_index_only(
    client: &Client,
    snapshot_kind: &str,
    project_code: &str,
    namespace_code: &str,
    limit: Option<i64>,
) -> Result<Vec<ObservabilitySnapshotRecord>> {
    let limit = limit.unwrap_or(i64::MAX);
    let rows = query_observability_snapshots_by_kind_for_scope_index_only(
        client,
        snapshot_kind,
        project_code,
        namespace_code,
        limit,
    )
    .await
    .with_context(|| {
        format!(
            "failed to list scoped observability snapshots (index-only api) for {}::{}::{}",
            snapshot_kind, project_code, namespace_code
        )
    })?;
    Ok(rows)
}

async fn query_observability_snapshots_by_kind_for_scope_index_only(
    client: &Client,
    snapshot_kind: &str,
    project_code: &str,
    namespace_code: &str,
    limit: i64,
) -> Result<Vec<ObservabilitySnapshotRecord>> {
    let rows = client
        .query(
            r#"
            SELECT
                snapshot_id,
                snapshot_kind,
                payload,
                (EXTRACT(EPOCH FROM created_at) * 1000)::bigint AS created_at_epoch_ms
            FROM ami.observability_snapshots
            WHERE snapshot_kind = $1
              AND scope_project_code = $2
              AND scope_namespace_code = $3
            ORDER BY COALESCE(captured_at_epoch_ms, (EXTRACT(EPOCH FROM created_at) * 1000)::bigint) DESC,
                     created_at DESC
            LIMIT $4
            "#,
            &[&snapshot_kind, &project_code, &namespace_code, &limit],
        )
        .await?;
    Ok(rows
        .into_iter()
        .map(|row| ObservabilitySnapshotRecord {
            snapshot_id: row.get(0),
            snapshot_kind: row.get(1),
            payload: row.get(2),
            created_at_epoch_ms: row.get(3),
        })
        .collect())
}

pub async fn list_latest_observability_snapshots_by_payload_string_field(
    client: &Client,
    snapshot_kind: &str,
    payload_root: &str,
    field_name: &str,
    values: &[String],
) -> Result<Vec<ObservabilitySnapshotRecord>> {
    if values.is_empty() {
        return Ok(Vec::new());
    }
    let rows = client
        .query(
            r#"
            SELECT
                snapshot_id,
                snapshot_kind,
                payload,
                (EXTRACT(EPOCH FROM created_at) * 1000)::bigint AS created_at_epoch_ms
            FROM ami.observability_snapshots
            WHERE snapshot_kind = $1
              AND payload #>> ARRAY[$2, $3] = ANY($4::text[])
            ORDER BY created_at DESC
            "#,
            &[&snapshot_kind, &payload_root, &field_name, &values],
        )
        .await
        .context("failed to list latest observability snapshots by payload string field")?;
    Ok(rows
        .into_iter()
        .map(|row| ObservabilitySnapshotRecord {
            snapshot_id: row.get(0),
            snapshot_kind: row.get(1),
            payload: row.get(2),
            created_at_epoch_ms: row.get(3),
        })
        .collect())
}

pub async fn list_observability_snapshots_by_payload_text_array_overlap(
    client: &Client,
    snapshot_kind: &str,
    payload_root: &str,
    field_name: &str,
    values: &[String],
) -> Result<Vec<ObservabilitySnapshotRecord>> {
    if values.is_empty() {
        return Ok(Vec::new());
    }
    let rows = client
        .query(
            r#"
            SELECT
                snapshot_id,
                snapshot_kind,
                payload,
                (EXTRACT(EPOCH FROM created_at) * 1000)::bigint AS created_at_epoch_ms
            FROM ami.observability_snapshots
            WHERE snapshot_kind = $1
              AND EXISTS (
                    SELECT 1
                    FROM jsonb_array_elements_text(
                        COALESCE(payload #> ARRAY[$2, $3], '[]'::jsonb)
                    ) AS item(value)
                    WHERE item.value = ANY($4::text[])
                )
            ORDER BY created_at DESC
            "#,
            &[&snapshot_kind, &payload_root, &field_name, &values],
        )
        .await
        .context("failed to list observability snapshots by payload text array overlap")?;
    Ok(rows
        .into_iter()
        .map(|row| ObservabilitySnapshotRecord {
            snapshot_id: row.get(0),
            snapshot_kind: row.get(1),
            payload: row.get(2),
            created_at_epoch_ms: row.get(3),
        })
        .collect())
}

pub async fn list_scoped_observability_snapshots_by_kinds(
    client: &Client,
    kinds: &[&str],
    project_code: &str,
    namespace_code: &str,
    limit: Option<i64>,
) -> Result<Vec<ObservabilitySnapshotRecord>> {
    if kinds.is_empty() {
        return Ok(Vec::new());
    }
    let limit = limit.unwrap_or(i64::MAX);
    let rows = client
        .query(
            r#"
            SELECT
                snapshot_id,
                snapshot_kind,
                payload,
                (EXTRACT(EPOCH FROM created_at) * 1000)::bigint AS created_at_epoch_ms
            FROM ami.observability_snapshots
            WHERE snapshot_kind = ANY($1::text[])
              AND scope_project_code = $2
              AND scope_namespace_code = $3
            ORDER BY created_at DESC
            LIMIT $4
            "#,
            &[&kinds, &project_code, &namespace_code, &limit],
        )
        .await
        .context("failed to list scoped observability snapshots by kinds")?;
    Ok(rows
        .into_iter()
        .map(|row| ObservabilitySnapshotRecord {
            snapshot_id: row.get(0),
            snapshot_kind: row.get(1),
            payload: row.get(2),
            created_at_epoch_ms: row.get(3),
        })
        .collect())
}

pub async fn latest_clean_benchmark_snapshot_payload(
    client: &Client,
    snapshot_kind: &str,
    payload_root: &str,
) -> Result<Option<Value>> {
    let row = client
        .query_opt(
            r#"
            SELECT payload
            FROM ami.observability_snapshots
            WHERE snapshot_kind = $1
              AND jsonb_typeof(payload -> $2) = 'object'
              AND COALESCE((payload -> $2 ->> 'record_live_context')::boolean, false) = false
              AND COALESCE((payload -> $2 ->> 'publish_benchmark_snapshot')::boolean, true) = true
              AND COALESCE(payload -> '_observability' ->> 'source_class', 'benchmark') = 'benchmark'
            ORDER BY created_at DESC
            LIMIT 1
            "#,
            &[&snapshot_kind, &payload_root],
        )
        .await
        .context("failed to fetch latest clean benchmark snapshot payload")?;
    Ok(row.map(|row| row.get(0)))
}

pub async fn summarize_observability_snapshots_by_kinds(
    client: &Client,
    kinds: &[&str],
) -> Result<Vec<ObservabilitySnapshotKindSummary>> {
    if kinds.is_empty() {
        return Ok(Vec::new());
    }
    let rows = client
        .query(
            r#"
            SELECT
                snapshot_kind,
                COUNT(*)::bigint AS snapshots_count,
                MAX((EXTRACT(EPOCH FROM created_at) * 1000)::bigint) AS latest_created_at_epoch_ms
            FROM ami.observability_snapshots
            WHERE snapshot_kind = ANY($1::text[])
            GROUP BY snapshot_kind
            ORDER BY snapshot_kind ASC
            "#,
            &[&kinds],
        )
        .await
        .context("failed to summarize observability snapshots by kinds")?;
    Ok(rows
        .into_iter()
        .map(|row| ObservabilitySnapshotKindSummary {
            snapshot_kind: row.get(0),
            snapshots_count: row.get(1),
            latest_created_at_epoch_ms: row.get(2),
        })
        .collect())
}

pub async fn list_observability_snapshots_older_than(
    client: &Client,
    cutoff_epoch_ms: i64,
    limit: Option<i64>,
) -> Result<Vec<ObservabilityRetentionCandidate>> {
    let limit = limit.unwrap_or(i64::MAX);
    let rows = client
        .query(
            r#"
            SELECT
                snapshot_id,
                snapshot_kind,
                payload,
                COALESCE(source_kind, '') AS source_kind,
                COALESCE(source_class, '') AS source_class,
                (EXTRACT(EPOCH FROM created_at) * 1000)::bigint AS created_at_epoch_ms,
                captured_at_epoch_ms
            FROM ami.observability_snapshots
            WHERE COALESCE(
                    captured_at_epoch_ms,
                    (EXTRACT(EPOCH FROM created_at) * 1000)::bigint
                  ) <= $1
            ORDER BY COALESCE(
                    captured_at_epoch_ms,
                    (EXTRACT(EPOCH FROM created_at) * 1000)::bigint
                  ) ASC
            LIMIT $2
            "#,
            &[&cutoff_epoch_ms, &limit],
        )
        .await
        .context("failed to list aged observability snapshots")?;
    Ok(rows
        .into_iter()
        .map(|row| ObservabilityRetentionCandidate {
            snapshot_id: row.get(0),
            snapshot_kind: row.get(1),
            payload: row.get(2),
            source_kind: row.get(3),
            source_class: row.get(4),
            created_at_epoch_ms: row.get(5),
            captured_at_epoch_ms: row.get(6),
        })
        .collect())
}

pub async fn delete_observability_snapshots_by_ids(
    client: &Client,
    snapshot_ids: &[Uuid],
) -> Result<u64> {
    if snapshot_ids.is_empty() {
        return Ok(0);
    }
    client
        .execute(
            r#"
            DELETE FROM ami.observability_snapshots
            WHERE snapshot_id = ANY($1::uuid[])
            "#,
            &[&snapshot_ids],
        )
        .await
        .context("failed to delete observability snapshots by ids")
}

pub async fn update_observability_snapshot_payload(
    client: &Client,
    snapshot_id: &Uuid,
    payload: &Value,
) -> Result<()> {
    let row = client
        .query_opt(
            r#"
            SELECT snapshot_kind, payload
            FROM ami.observability_snapshots
            WHERE snapshot_id = $1
            "#,
            &[snapshot_id],
        )
        .await
        .context("failed to load observability snapshot metadata before update")?
        .ok_or_else(|| anyhow!("observability snapshot not found: {snapshot_id}"))?;
    let snapshot_kind: String = row.get(0);
    let existing_payload: Value = row.get(1);
    let (stored_payload, meta) = prepare_observability_payload(&snapshot_kind, payload)?;
    match validate_observability_update(
        &snapshot_kind,
        snapshot_id,
        &existing_payload,
        &stored_payload,
    )? {
        false => return Ok(()),
        true => {}
    }
    client
        .execute(
            r#"
            UPDATE ami.observability_snapshots
            SET payload = $2,
                event_key = $3,
                source_event_id = $4,
                source_kind = $5,
                source_class = $6,
                scope_project_code = $7,
                scope_namespace_code = $8,
                captured_at_epoch_ms = $9,
                payload_sha256 = $10,
                last_seen_at = now()
            WHERE snapshot_id = $1
            "#,
            &[
                snapshot_id,
                &stored_payload,
                &meta.event_key,
                &meta.source_event_id,
                &meta.source_kind,
                &meta.source_class,
                &meta.scope_project_code,
                &meta.scope_namespace_code,
                &meta.captured_at_epoch_ms,
                &meta.payload_sha256,
            ],
        )
        .await
        .context("failed to update observability snapshot payload")?;
    Ok(())
}

pub async fn get_observability_snapshot_record(
    client: &Client,
    snapshot_id: &Uuid,
) -> Result<Option<ObservabilitySnapshotRecord>> {
    let row = client
        .query_opt(
            r#"
            SELECT
                snapshot_id,
                snapshot_kind,
                payload,
                (EXTRACT(EPOCH FROM created_at) * 1000)::bigint AS created_at_epoch_ms
            FROM ami.observability_snapshots
            WHERE snapshot_id = $1
            "#,
            &[snapshot_id],
        )
        .await
        .context("failed to fetch observability snapshot by id")?;
    Ok(row.map(|row| ObservabilitySnapshotRecord {
        snapshot_id: row.get(0),
        snapshot_kind: row.get(1),
        payload: row.get(2),
        created_at_epoch_ms: row.get(3),
    }))
}

pub(super) fn observability_conflict_error(
    snapshot_kind: &str,
    meta: &ObservabilityInsertMeta,
    existing_snapshot_id: Uuid,
    existing_source_event_id: Option<&str>,
    existing_captured_at_epoch_ms: Option<i64>,
) -> anyhow::Error {
    if meta.source_event_id.is_some()
        && meta.source_event_id.as_deref() == existing_source_event_id
        && meta
            .captured_at_epoch_ms
            .zip(existing_captured_at_epoch_ms)
            .is_some_and(|(incoming, existing)| incoming > existing)
    {
        anyhow!(
            "observability anti-replay blocked newer divergent payload for immutable event {} :: {}",
            snapshot_kind,
            meta.event_key
        )
    } else {
        anyhow!(
            "observability idempotency blocked divergent payload for {} :: {} (existing snapshot {})",
            snapshot_kind,
            meta.event_key,
            existing_snapshot_id
        )
    }
}

fn extract_sqlstate_code(error: &Error) -> Option<String> {
    if let Some(pg_error) = error.downcast_ref::<tokio_postgres::Error>() {
        return pg_error
            .as_db_error()
            .map(|db_error| db_error.code().code().to_string());
    }
    for cause in error.chain() {
        if let Some(pg_error) = cause.downcast_ref::<tokio_postgres::Error>() {
            return pg_error
                .as_db_error()
                .map(|db_error| db_error.code().code().to_string());
        }
    }
    None
}

#[cfg(test)]
fn forced_observability_insert_error_for_tests(
    snapshot_kind: &str,
    event_key: &str,
    write_may_have_succeeded: bool,
) -> Option<ObservabilityInsertError> {
    let raw = std::env::var("AMAI_TEST_FORCE_OBSERVABILITY_INSERT_FAILURE").ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let (phase_raw, sqlstate_raw) = trimmed.split_once(':').unwrap_or((trimmed, "XX000"));
    let sqlstate_code = Some(sqlstate_raw.trim().to_string());
    match phase_raw.trim() {
        "before_write" if !write_may_have_succeeded => Some(
            ObservabilityInsertError::before_write_with_sqlstate(
                snapshot_kind,
                event_key,
                sqlstate_code,
                anyhow!(
                    "forced observability insert failure for tests snapshot_kind={} event_key={} phase=before_write sqlstate={}",
                    snapshot_kind,
                    event_key,
                    sqlstate_raw.trim()
                ),
            ),
        ),
        "outcome_unknown_after_write" if write_may_have_succeeded => Some(
            ObservabilityInsertError::outcome_unknown_after_write(
                snapshot_kind,
                event_key,
                sqlstate_code,
                anyhow!(
                    "forced observability insert failure for tests snapshot_kind={} event_key={} phase=outcome_unknown_after_write sqlstate={}",
                    snapshot_kind,
                    event_key,
                    sqlstate_raw.trim()
                ),
            ),
        ),
        _ => None,
    }
}

pub(super) fn validate_observability_update(
    snapshot_kind: &str,
    snapshot_id: &Uuid,
    existing_payload: &Value,
    stored_payload: &Value,
) -> Result<bool> {
    if existing_payload == stored_payload {
        return Ok(false);
    }
    if existing_payload["_observability"]["immutable_snapshot"].as_bool() == Some(true) {
        return Err(anyhow!(
            "observability snapshot is immutable and cannot be updated: {snapshot_kind} / {snapshot_id}"
        ));
    }
    Ok(true)
}

pub(super) fn prepare_observability_payload(
    snapshot_kind: &str,
    payload: &Value,
) -> Result<(Value, ObservabilityInsertMeta)> {
    validate_snapshot_payload_shape(snapshot_kind, payload)?;
    let payload_sha256 = hex_sha256(
        &serde_json::to_vec(payload).context("failed to serialize observability payload")?,
    );
    let source_event_id = extract_first_string(
        payload,
        &[
            &["_observability", "source_event_id"],
            &["token_budget_event", "event_id"],
            &["working_state_event", "event_id"],
            &["working_state_event", "context_pack_id"],
            &["context_pack_id"],
        ],
    );
    let source_kind = extract_first_string(
        payload,
        &[
            &["_observability", "source_kind"],
            &["token_budget_event", "source_kind"],
            &["working_state_event", "source_kind"],
            &["continuity_handoff", "source_kind"],
        ],
    )
    .unwrap_or_else(|| snapshot_kind.to_string());
    let source_class = observability_source_class(snapshot_kind, payload).to_string();
    let scope_project_code = extract_first_string(
        payload,
        &[
            &["_observability", "scope_project_code"],
            &["project", "code"],
            &["working_state_restore", "project", "code"],
            &["working_state_event", "project", "code"],
            &["continuity_import", "project", "code"],
            &["continuity_handoff", "project", "code"],
            &["token_budget_event", "project_code"],
            &["token_budget_event", "project"],
            &["benchmark", "project"],
            &["accuracy_verification", "project"],
            &["load_verification", "project"],
            &["cold_benchmark", "project"],
        ],
    );
    let scope_namespace_code = extract_first_string(
        payload,
        &[
            &["_observability", "scope_namespace_code"],
            &["namespace", "code"],
            &["working_state_restore", "namespace", "code"],
            &["working_state_event", "namespace", "code"],
            &["continuity_import", "namespace", "code"],
            &["continuity_handoff", "namespace", "code"],
            &["token_budget_event", "namespace_code"],
            &["token_budget_event", "namespace"],
            &["benchmark", "namespace"],
            &["accuracy_verification", "namespace"],
            &["load_verification", "namespace"],
        ],
    );
    let captured_at_epoch_ms = extract_first_i64(
        payload,
        &[
            &["_observability", "captured_at_epoch_ms"],
            &["captured_at_epoch_ms"],
            &["working_state_restore", "captured_at_epoch_ms"],
            &["working_state_event", "recorded_at_epoch_ms"],
            &["token_budget_event", "created_at_epoch_ms"],
            &["continuity_import", "imported_at_epoch_ms"],
            &["continuity_thread_index", "captured_at_epoch_ms"],
            &["continuity_handoff", "captured_at_epoch_ms"],
            &["benchmark", "captured_at_epoch_ms"],
            &["accuracy_verification", "captured_at_epoch_ms"],
            &["load_verification", "captured_at_epoch_ms"],
            &["cold_benchmark", "captured_at_epoch_ms"],
        ],
    );
    let event_key = source_event_id
        .clone()
        .unwrap_or_else(|| format!("sha256:{payload_sha256}"));
    let policy_meta =
        observability_policy::policy_metadata(snapshot_kind, payload, &source_kind, &source_class)?;
    let meta = ObservabilityInsertMeta {
        event_key: event_key.clone(),
        source_event_id,
        source_kind: source_kind.clone(),
        source_class: source_class.clone(),
        scope_project_code: scope_project_code.clone(),
        scope_namespace_code: scope_namespace_code.clone(),
        captured_at_epoch_ms,
        payload_sha256: payload_sha256.clone(),
    };

    let mut stored_payload = payload.clone();
    if let Some(object) = stored_payload.as_object_mut() {
        let mut observability_meta = object
            .get("_observability")
            .cloned()
            .filter(|value| value.is_object())
            .unwrap_or_else(|| json!({}));
        let observability_meta_object = observability_meta
            .as_object_mut()
            .expect("observability meta initialized as object");
        for (key, value) in json!({
            "snapshot_kind": snapshot_kind,
            "event_key": event_key,
            "source_event_id": meta.source_event_id,
            "source_kind": source_kind,
            "source_class": source_class,
            "scope_project_code": scope_project_code,
            "scope_namespace_code": scope_namespace_code,
            "captured_at_epoch_ms": captured_at_epoch_ms,
            "payload_sha256": payload_sha256,
            "replay_protected": meta.source_event_id.is_some(),
        })
        .as_object()
        .expect("observability base metadata object")
        {
            observability_meta_object.insert(key.clone(), value.clone());
        }
        for (key, value) in policy_meta
            .as_object()
            .expect("observability policy metadata object")
        {
            observability_meta_object.insert(key.clone(), value.clone());
        }
        object.insert("_observability".to_string(), observability_meta);
    }

    Ok((stored_payload, meta))
}

fn validate_snapshot_payload_shape(snapshot_kind: &str, payload: &Value) -> Result<()> {
    if snapshot_kind != "working_state_restore" {
        return Ok(());
    }
    let restore = payload
        .get("working_state_restore")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            anyhow!("working_state_restore observability payload must include object root")
        })?;
    let project_code = restore
        .get("project")
        .and_then(Value::as_object)
        .and_then(|project| project.get("code"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            anyhow!("working_state_restore observability payload must include project.code")
        })?;
    let namespace_code = restore
        .get("namespace")
        .and_then(Value::as_object)
        .and_then(|namespace| namespace.get("code"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            anyhow!("working_state_restore observability payload must include namespace.code")
        })?;
    let _ = (project_code, namespace_code);
    if restore.get("captured_at_epoch_ms").is_some() && !restore["captured_at_epoch_ms"].is_i64() {
        return Err(anyhow!(
            "working_state_restore observability payload captured_at_epoch_ms must be integer"
        ));
    }
    if let Some(source_event_id) = payload["_observability"]["source_event_id"].as_str() {
        let authoritative_event_id = restore
            .get("state_lineage")
            .and_then(Value::as_object)
            .and_then(|lineage| lineage.get("authoritative_event_id"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                anyhow!(
                    "working_state_restore observability payload must include state_lineage.authoritative_event_id when _observability.source_event_id is set"
                )
            })?;
        if authoritative_event_id != source_event_id.trim() {
            return Err(anyhow!(
                "working_state_restore observability payload source_event_id must match authoritative_event_id"
            ));
        }
    }
    Ok(())
}

pub(super) fn observability_source_class(snapshot_kind: &str, payload: &Value) -> &'static str {
    if payload["load_verification"]["record_live_context"].as_bool() == Some(true)
        || payload["load_verification"]["publish_benchmark_snapshot"].as_bool() == Some(false)
    {
        return "live_context";
    }
    match snapshot_kind {
        "retrieval_benchmark_hot"
        | "retrieval_benchmark_cold"
        | "retrieval_load_hot"
        | "retrieval_load_cold"
        | "retrieval_accuracy"
        | "continuity_verification"
        | "cold_path_benchmark"
        | "token_benchmark"
        | "token_benchmark_suite"
        | "text_compare"
        | "mcp_task_matrix"
        | "memory_task_matrix" => "benchmark",
        "system_snapshot" => "live_system",
        _ => "operational",
    }
}

fn extract_first_string(value: &Value, paths: &[&[&str]]) -> Option<String> {
    paths.iter().find_map(|path| {
        value_at_path(value, path).and_then(|node| node.as_str().map(ToOwned::to_owned))
    })
}

fn extract_first_i64(value: &Value, paths: &[&[&str]]) -> Option<i64> {
    paths.iter().find_map(|path| {
        value_at_path(value, path).and_then(|node| {
            node.as_i64()
                .or_else(|| node.as_u64().and_then(|number| i64::try_from(number).ok()))
        })
    })
}

fn value_at_path<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    let mut current = value;
    for segment in path {
        current = current.get(*segment)?;
    }
    Some(current)
}

fn hex_sha256(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}
