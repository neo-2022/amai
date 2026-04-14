async fn ensure_context_pack_persisted(
    cfg: &AppConfig,
    db: &mut Client,
    args: &ContextPackArgs,
    prepared: &mut PreparedContextPack,
) -> Result<()> {
    if prepared.durably_persisted {
        return Ok(());
    }
    let persist_started = Instant::now();
    persist_context_pack(cfg, db, args, prepared).await?;
    prepared.durably_persisted = true;
    prepared.stats.timings.persist_ms = persist_started.elapsed().as_millis();
    Ok(())
}

async fn persist_context_pack(
    cfg: &AppConfig,
    db: &mut Client,
    args: &ContextPackArgs,
    prepared: &mut PreparedContextPack,
) -> Result<()> {
    let payload_sha256 = hex_sha256(prepared.payload_json.as_bytes());
    let object_key = format!(
        "context-packs/{}/{}/{}.json",
        prepared.project.code, prepared.effective_mode, payload_sha256
    );
    let context_pack_record = postgres::ContextPackInsert {
        context_pack_id: prepared.context_pack_id,
        project_id: prepared.project.project_id,
        namespace_id: prepared.namespace_id,
        retrieval_mode: &prepared.effective_mode,
        query_text: &args.query,
        visible_projects: &prepared.visible_projects_json,
        payload: prepared.payload.as_ref(),
        artifact_ref_id: None,
    };
    prepared.artifact_bucket = Some(cfg.s3_bucket_context.clone());
    prepared.artifact_object_key = Some(object_key.clone());

    postgres::insert_context_pack_pending_artifact(
        db,
        &context_pack_record,
        &cfg.s3_bucket_context,
        &object_key,
    )
    .await?;
    if should_persist_retrieval_trace(&args.token_source_kind) {
        let workspace_id =
            postgres::get_workspace_id_for_project(db, prepared.project.project_id).await?;
        let retrieval_trace = build_retrieval_trace_insert(workspace_id, prepared, args);
        postgres::create_retrieval_trace(db, &retrieval_trace).await?;
    }
    prepared.artifact_state = Some("pending".to_string());
    Ok(())
}

pub async fn materialize_pending_context_pack_artifacts(
    cfg: &AppConfig,
    db: &mut Client,
    limit: i64,
) -> Result<()> {
    let claimed = postgres::claim_pending_context_pack_artifacts(db, limit).await?;
    let claimed_rows = claimed.len();
    let mut grouped: HashMap<(String, String), Vec<postgres::PendingContextPackArtifactRecord>> =
        HashMap::new();
    for record in claimed {
        grouped
            .entry((record.bucket.clone(), record.object_key.clone()))
            .or_default()
            .push(record);
    }

    let s3_client = if grouped.is_empty() {
        None
    } else {
        Some(s3::connect(cfg).await?)
    };
    let mut materialized_rows = 0_u64;
    let mut failed_rows = 0_u64;
    let mut materialized_groups = 0_u64;
    let mut failed_groups = 0_u64;

    for ((bucket, object_key), records) in grouped {
        let Some(first) = records.first() else {
            continue;
        };
        let payload_json = serde_json::to_string(&first.payload)?;
        let payload_sha256 = hex_sha256(payload_json.as_bytes());
        let metadata = json!({
            "artifact_role": "context_pack",
            "payload_sha256": payload_sha256
        });
        let upload_result = s3::put_json_object(
            s3_client
                .as_ref()
                .expect("s3 client exists when pending groups exist"),
            &bucket,
            &object_key,
            &payload_json,
        )
        .await;

        match upload_result {
            Ok(()) => {
                let artifact_ref_result = postgres::insert_artifact_ref(
                    db,
                    &postgres::ArtifactRefInsert {
                        project_id: first.project_id,
                        namespace_id: first.namespace_id,
                        artifact_kind: "context_pack",
                        bucket: &bucket,
                        object_key: &object_key,
                        content_type: Some("application/json"),
                        source_kind: Some("context_pack_materialization"),
                        source_event_ids: None,
                        message_refs: None,
                        evidence_span: Some(&json!({
                            "artifact_role": "context_pack",
                            "bucket": bucket,
                            "object_key": object_key,
                            "context_pack_id": first.context_pack_id,
                        })),
                        derivation_kind: Some("extract"),
                        schema_version: Some("artifact-ref-envelope-v1"),
                        metadata: &metadata,
                    },
                )
                .await;

                match artifact_ref_result {
                    Ok(artifact_ref_id) => {
                        materialized_rows += postgres::mark_context_pack_artifacts_materialized(
                            db,
                            &bucket,
                            &object_key,
                            artifact_ref_id,
                        )
                        .await?;
                        materialized_groups += 1;
                    }
                    Err(error) => {
                        let error_text = format!("{error:#}");
                        for record in &records {
                            postgres::mark_context_pack_artifact_failed(
                                db,
                                record.context_pack_id,
                                &error_text,
                            )
                            .await?;
                            failed_rows += 1;
                        }
                        failed_groups += 1;
                    }
                }
            }
            Err(error) => {
                let error_text = format!("{error:#}");
                for record in &records {
                    postgres::mark_context_pack_artifact_failed(
                        db,
                        record.context_pack_id,
                        &error_text,
                    )
                    .await?;
                    failed_rows += 1;
                }
                failed_groups += 1;
            }
        }
    }

    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "claimed_rows": claimed_rows,
            "claimed_groups": materialized_groups + failed_groups,
            "materialized_rows": materialized_rows,
            "materialized_groups": materialized_groups,
            "failed_rows": failed_rows,
            "failed_groups": failed_groups
        }))?
    );
    Ok(())
}

fn cache_context_pack_entry(
    cfg: &AppConfig,
    args: &ContextPackArgs,
    prepared: &PreparedContextPack,
    durably_persisted: bool,
) -> Result<()> {
    if args.disable_cache {
        return Ok(());
    }
    let edge_cache_path = edge_cache::ensure(&cfg.edge_cache_path)?;
    edge_cache::cache_context_pack(
        &edge_cache_path,
        &prepared.context_pack_id.to_string(),
        &prepared.project.code,
        &prepared.effective_mode,
        &prepared.payload_json,
    )?;
    edge_cache::upsert_context_pack_cache_entry(
        &edge_cache_path,
        &edge_cache::ContextPackCacheRecord {
            cache_key: &prepared.cache_key,
            scope_signature: &prepared.scope_signature,
            context_pack_id: &prepared.context_pack_id.to_string(),
            project_code: &prepared.project.code,
            namespace_code: &args.namespace,
            retrieval_mode: &prepared.effective_mode,
            payload_json: prepared.payload_json.as_ref(),
            durably_persisted,
        },
    )?;
    let local_entry = local_entry_from_prepared(prepared, durably_persisted);
    local_context_pack_cache_put(&prepared.cache_key, &prepared.scope_signature, &local_entry)?;
    let fast_cache_key = fast_cache_key(cfg, args, &prepared.effective_mode);
    local_fast_context_pack_cache_put(fast_cache_key, &local_entry)?;
    edge_cache::upsert_fast_context_pack_cache_entry(
        &edge_cache_path,
        fast_cache_key,
        &prepared.context_pack_id.to_string(),
        prepared.payload_json.as_ref(),
        durably_persisted,
        local_entry.cached_at_epoch_ms,
    )?;
    if args.retrieval_mode.is_none() {
        let default_fast_cache_key = selected_fast_cache_key(cfg, args);
        local_fast_context_pack_cache_put(default_fast_cache_key, &local_entry)?;
        edge_cache::upsert_fast_context_pack_cache_entry(
            &edge_cache_path,
            default_fast_cache_key,
            &prepared.context_pack_id.to_string(),
            prepared.payload_json.as_ref(),
            durably_persisted,
            local_entry.cached_at_epoch_ms,
        )?;
    }
    Ok(())
}
