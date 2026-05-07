use super::*;

pub(super) const PROCEDURAL_BENCHMARK_HISTORY_LIMIT: i64 = 12;

fn history_snapshot_captured_at_epoch_ms(
    snapshot: &postgres::ObservabilitySnapshotRecord,
    payload_root: &str,
) -> u64 {
    snapshot.payload[payload_root]["captured_at_epoch_ms"]
        .as_u64()
        .or_else(|| snapshot.payload["_observability"]["captured_at_epoch_ms"].as_u64())
        .unwrap_or(snapshot.created_at_epoch_ms.max(0) as u64)
}

pub(super) async fn procedural_benchmark_history_surface(db: &Client) -> Result<Value> {
    let snapshots = postgres::list_observability_snapshots_by_kind_for_scope_index_only(
        db,
        "procedural_benchmark",
        "amai",
        "benchmark",
        Some(PROCEDURAL_BENCHMARK_HISTORY_LIMIT),
    )
    .await?;
    let history_rows: Vec<Value> = snapshots
        .into_iter()
        .rev()
        .map(|snapshot| {
            let payload = &snapshot.payload["procedural_benchmark"];
            let with_summary = &payload["benchmark_line_summaries"]["with_amai"];
            let without_summary = &payload["benchmark_line_summaries"]["without_amai_but_measuring"];
            json!({
                "benchmark_run_id": payload["benchmark_run_id"].clone(),
                "captured_at_epoch_ms": history_snapshot_captured_at_epoch_ms(&snapshot, "procedural_benchmark"),
                "benchmark_run_state": payload["benchmark_run_state"].clone(),
                "benchmark_run_state_ru": payload["benchmark_run_state_ru"].clone(),
                "with_amai_pass_percent": with_summary["pass_percent"].clone(),
                "without_amai_pass_percent": without_summary["pass_percent"].clone(),
                "with_amai_point_count": with_summary["point_count"].clone(),
                "without_amai_point_count": without_summary["point_count"].clone(),
                "without_amai_series_available": payload["summary"]["without_amai_series_available"].clone()
            })
        })
        .collect();
    let with_amai_pass_percent_series: Vec<Value> = history_rows
        .iter()
        .filter_map(|row| {
            Some(json!({
                "benchmark_run_id": row["benchmark_run_id"].clone(),
                "captured_at_epoch_ms": row["captured_at_epoch_ms"].clone(),
                "pass_percent": row["with_amai_pass_percent"].as_f64()?
            }))
        })
        .collect();
    let without_amai_pass_percent_series: Vec<Value> = history_rows
        .iter()
        .filter_map(|row| {
            Some(json!({
                "benchmark_run_id": row["benchmark_run_id"].clone(),
                "captured_at_epoch_ms": row["captured_at_epoch_ms"].clone(),
                "pass_percent": row["without_amai_pass_percent"].as_f64()?
            }))
        })
        .collect();
    Ok(json!({
        "snapshot_kind": "procedural_benchmark",
        "scope_project_code": "amai",
        "scope_namespace_code": "benchmark",
        "history_limit": PROCEDURAL_BENCHMARK_HISTORY_LIMIT,
        "history_count": history_rows.len(),
        "with_amai_history_count": with_amai_pass_percent_series.len(),
        "without_amai_history_count": without_amai_pass_percent_series.len(),
        "history_rows": history_rows,
        "with_amai_pass_percent_series": with_amai_pass_percent_series,
        "without_amai_pass_percent_series": without_amai_pass_percent_series
    }))
}

#[derive(Debug, Serialize)]
struct GuardrailCheck {
    name: &'static str,
    status: &'static str,
    details: Value,
}

pub(super) async fn collect_guardrail_report(db: &Client, prefix: &str) -> Result<Value> {
    let mut checks = Vec::new();
    checks.push(prove_direct_sql_working_state_event_id(db, prefix).await?);
    checks.push(prove_direct_sql_benchmark_contamination_block(db, prefix).await?);
    checks.push(prove_idempotent_replay_counter(db, prefix).await?);
    checks.push(prove_newer_divergent_payload_is_anti_replay(db, prefix).await?);
    checks.push(prove_immutable_snapshot_update_is_blocked(db, prefix).await?);
    Ok(json!({
        "status": "pass",
        "guardrails": checks,
    }))
}

pub(super) async fn cleanup_guardrail_rows(db: &Client, prefix: &str) -> Result<()> {
    let like = format!("{prefix}%");
    db.execute(
        r#"
        DELETE FROM ami.observability_snapshots
        WHERE event_key LIKE $1
           OR COALESCE(source_event_id, '') LIKE $1
        "#,
        &[&like],
    )
    .await
    .context("failed to cleanup observability guardrail proof rows")?;
    Ok(())
}

async fn prove_direct_sql_working_state_event_id(
    db: &Client,
    prefix: &str,
) -> Result<GuardrailCheck> {
    let event_id = format!("{prefix}-working-state");
    let payload = json!({
        "working_state_event": {
            "event_id": event_id,
            "context_pack_id": format!("{prefix}-legacy-context-pack"),
            "source_kind": "context_pack",
            "project": {
                "code": "amai"
            },
            "namespace": {
                "code": "default"
            },
            "recorded_at_epoch_ms": 101
        }
    });
    let row = db
        .query_one(
            r#"
            INSERT INTO ami.observability_snapshots(snapshot_kind, payload)
            VALUES ($1, $2)
            RETURNING snapshot_id, event_key, source_event_id
            "#,
            &[&"working_state_event", &payload],
        )
        .await
        .context("failed to insert direct-SQL working_state proof row")?;
    let snapshot_id: Uuid = row.get(0);
    let event_key: String = row.get(1);
    let source_event_id: Option<String> = row.get(2);
    if event_key != event_id || source_event_id.as_deref() != Some(event_id.as_str()) {
        return Err(anyhow!(
            "working_state direct SQL proof expected event_id={} but stored event_key={} source_event_id={:?}",
            event_id,
            event_key,
            source_event_id
        ));
    }
    Ok(GuardrailCheck {
        name: "direct_sql_working_state_event_id",
        status: "pass",
        details: json!({
            "snapshot_id": snapshot_id,
            "event_key": event_key,
            "source_event_id": source_event_id,
        }),
    })
}

async fn prove_direct_sql_benchmark_contamination_block(
    db: &Client,
    prefix: &str,
) -> Result<GuardrailCheck> {
    let event_id = format!("{prefix}-contamination");
    let payload = json!({
        "_observability": {
            "source_event_id": event_id
        },
        "load_verification": {
            "project": "amai",
            "namespace": "default",
            "captured_at_epoch_ms": 202,
            "record_live_context": true,
            "publish_benchmark_snapshot": false
        }
    });
    let error = db
        .execute(
            r#"
            INSERT INTO ami.observability_snapshots(snapshot_kind, payload)
            VALUES ($1, $2)
            "#,
            &[&"retrieval_load_hot", &payload],
        )
        .await
        .expect_err("contaminated benchmark insert must fail");
    let message = postgres_error_message(&error);
    if !message.contains("benchmark lane contamination blocked") {
        return Err(anyhow!(
            "unexpected benchmark contamination error: {message}"
        ));
    }
    Ok(GuardrailCheck {
        name: "direct_sql_benchmark_contamination_block",
        status: "pass",
        details: json!({
            "error": message,
        }),
    })
}

fn postgres_error_message(error: &tokio_postgres::Error) -> String {
    if let Some(db_error) = error.as_db_error() {
        let mut message = db_error.message().to_string();
        if let Some(detail) = db_error.detail() {
            message.push_str(&format!(" | detail: {detail}"));
        }
        if let Some(hint) = db_error.hint() {
            message.push_str(&format!(" | hint: {hint}"));
        }
        return message;
    }
    error.to_string()
}

async fn prove_idempotent_replay_counter(db: &Client, prefix: &str) -> Result<GuardrailCheck> {
    let event_id = format!("{prefix}-replay");
    let payload = json!({
        "_observability": {
            "source_event_id": event_id,
            "source_kind": "benchmark_run"
        },
        "benchmark": {
            "project": "project_alpha",
            "namespace": "default",
            "captured_at_epoch_ms": 303,
            "p95_ms": 0.5
        }
    });
    let first_snapshot_id =
        postgres::insert_observability_snapshot(db, "retrieval_benchmark_hot", &payload).await?;
    let replay_snapshot_id =
        postgres::insert_observability_snapshot(db, "retrieval_benchmark_hot", &payload).await?;
    let row = db
        .query_one(
            r#"
            SELECT replay_count
            FROM ami.observability_snapshots
            WHERE snapshot_id = $1
            "#,
            &[&first_snapshot_id],
        )
        .await
        .context("failed to fetch replay_count for observability proof row")?;
    let replay_count: i64 = row.get(0);
    if replay_snapshot_id != first_snapshot_id || replay_count != 1 {
        return Err(anyhow!(
            "idempotent replay proof expected same snapshot_id with replay_count=1, got first={} replay={} replay_count={}",
            first_snapshot_id,
            replay_snapshot_id,
            replay_count
        ));
    }
    Ok(GuardrailCheck {
        name: "idempotent_replay_counter",
        status: "pass",
        details: json!({
            "snapshot_id": first_snapshot_id,
            "replay_count": replay_count,
        }),
    })
}

async fn prove_newer_divergent_payload_is_anti_replay(
    db: &Client,
    prefix: &str,
) -> Result<GuardrailCheck> {
    let event_id = format!("{prefix}-anti-replay");
    let older = json!({
        "_observability": {
            "source_event_id": event_id,
            "source_kind": "benchmark_run"
        },
        "benchmark": {
            "project": "project_alpha",
            "namespace": "default",
            "captured_at_epoch_ms": 404,
            "p95_ms": 0.4
        }
    });
    let newer = json!({
        "_observability": {
            "source_event_id": event_id,
            "source_kind": "benchmark_run"
        },
        "benchmark": {
            "project": "project_alpha",
            "namespace": "default",
            "captured_at_epoch_ms": 405,
            "p95_ms": 0.9
        }
    });
    let snapshot_id =
        postgres::insert_observability_snapshot(db, "retrieval_benchmark_hot", &older).await?;
    let error = postgres::insert_observability_snapshot(db, "retrieval_benchmark_hot", &newer)
        .await
        .expect_err("newer divergent payload must trigger anti-replay");
    let message = format!("{error:#}");
    if !message.contains("observability anti-replay blocked newer divergent payload") {
        return Err(anyhow!("unexpected anti-replay error: {message}"));
    }
    Ok(GuardrailCheck {
        name: "newer_divergent_payload_is_anti_replay",
        status: "pass",
        details: json!({
            "snapshot_id": snapshot_id,
            "error": message,
        }),
    })
}

async fn prove_immutable_snapshot_update_is_blocked(
    db: &Client,
    prefix: &str,
) -> Result<GuardrailCheck> {
    let event_id = format!("{prefix}-immutable");
    let original = json!({
        "_observability": {
            "source_event_id": event_id,
            "source_kind": "benchmark_run"
        },
        "benchmark": {
            "project": "project_alpha",
            "namespace": "default",
            "captured_at_epoch_ms": 505,
            "p95_ms": 0.6
        }
    });
    let mut updated = original.clone();
    updated["benchmark"]["p95_ms"] = json!(1.2);
    let snapshot_id =
        postgres::insert_observability_snapshot(db, "retrieval_benchmark_hot", &original).await?;
    let error = postgres::update_observability_snapshot_payload(db, &snapshot_id, &updated)
        .await
        .expect_err("immutable benchmark snapshot update must fail");
    let message = format!("{error:#}");
    if !message.contains("observability snapshot is immutable and cannot be updated") {
        return Err(anyhow!("unexpected immutable update error: {message}"));
    }
    Ok(GuardrailCheck {
        name: "immutable_snapshot_update_is_blocked",
        status: "pass",
        details: json!({
            "snapshot_id": snapshot_id,
            "error": message,
        }),
    })
}
