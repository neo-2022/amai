use super::*;

pub(super) async fn collect_postgres_live(
    db: &tokio_postgres::Client,
    profile: &SnapshotProfile,
) -> Result<Value> {
    let max_connections = db
        .query_one("SHOW max_connections", &[])
        .await?
        .get::<_, String>(0)
        .parse::<u64>()
        .context("failed to parse postgres max_connections")?;
    let active_connections = db
        .query_one("SELECT COUNT(*)::bigint FROM pg_stat_activity", &[])
        .await?
        .get::<_, i64>(0) as u64;
    let row = db
        .query_one(
            r#"
            SELECT
                COALESCE(numbackends, 0)::bigint,
                COALESCE(xact_commit + xact_rollback, 0)::bigint,
                COALESCE(deadlocks, 0)::bigint
            FROM pg_stat_database
            WHERE datname = current_database()
            "#,
            &[],
        )
        .await?;
    let numbackends = row.get::<_, i64>(0) as u64;
    let transactions_total = row.get::<_, i64>(1) as u64;
    let deadlocks_total = row.get::<_, i64>(2) as u64;
    let wal_bytes_total = db
        .query_one(
            "SELECT COALESCE(wal_bytes::bigint, 0) FROM pg_stat_wal",
            &[],
        )
        .await?
        .get::<_, i64>(0) as u64;
    let replica_lag_seconds = db
        .query_one(
            r#"
            SELECT COALESCE(
                MAX(EXTRACT(EPOCH FROM COALESCE(replay_lag, flush_lag, write_lag))),
                0
            )::double precision
            FROM pg_stat_replication
            "#,
            &[],
        )
        .await?
        .get::<_, f64>(0);

    let mut probe_samples = Vec::with_capacity(profile.postgres_query_probe_iterations);
    for _ in 0..profile.postgres_query_probe_iterations {
        let started = Instant::now();
        db.query_one("SELECT 1", &[]).await?;
        probe_samples.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    let query_probe_p95_ms = percentile_f64(&probe_samples, 95);

    Ok(json!({
        "max_connections": max_connections,
        "active_connections": active_connections,
        "numbackends": numbackends,
        "connection_usage_ratio": ratio(active_connections, max_connections),
        "transactions_total": transactions_total,
        "deadlocks_total": deadlocks_total,
        "wal_bytes_total": wal_bytes_total,
        "replica_lag_seconds": replica_lag_seconds,
        "query_probe_p95_ms": query_probe_p95_ms,
        "query_probe_samples_ms": probe_samples,
    }))
}

pub(super) fn with_postgres_rates(current: &Value, previous: Option<&Value>) -> Value {
    let captured_at = current["captured_at_epoch_ms"].as_u64().unwrap_or_default();
    let prev_captured_at = previous.and_then(|value| value["captured_at_epoch_ms"].as_u64());
    let dt_ms = prev_captured_at
        .and_then(|prev| captured_at.checked_sub(prev))
        .unwrap_or_default();
    let dt_s = if dt_ms == 0 {
        None
    } else {
        Some(dt_ms as f64 / 1000.0)
    };

    let tx_per_sec = dt_s.and_then(|dt| {
        delta_rate(
            current["transactions_total"].as_f64().unwrap_or(0.0),
            previous.and_then(|value| value["postgres"]["transactions_total"].as_f64()),
            dt,
        )
    });
    let deadlocks_delta = counter_delta(
        current["deadlocks_total"].as_f64().unwrap_or(0.0),
        previous.and_then(|value| value["postgres"]["deadlocks_total"].as_f64()),
    );
    let deadlocks_per_sec = dt_s.and_then(|dt| {
        delta_rate(
            current["deadlocks_total"].as_f64().unwrap_or(0.0),
            previous.and_then(|value| value["postgres"]["deadlocks_total"].as_f64()),
            dt,
        )
    });
    let wal_bytes_per_sec = dt_s.and_then(|dt| {
        delta_rate(
            current["wal_bytes_total"].as_f64().unwrap_or(0.0),
            previous.and_then(|value| value["postgres"]["wal_bytes_total"].as_f64()),
            dt,
        )
    });

    let mut value = current.clone();
    if let Some(object) = value.as_object_mut() {
        object.insert(
            "transactions_per_sec".to_string(),
            tx_per_sec.map_or(Value::Null, Value::from),
        );
        object.insert(
            "deadlocks_delta".to_string(),
            deadlocks_delta.map_or(Value::Null, Value::from),
        );
        object.insert(
            "deadlocks_per_sec".to_string(),
            deadlocks_per_sec.map_or(Value::Null, Value::from),
        );
        object.insert(
            "wal_bytes_per_sec".to_string(),
            wal_bytes_per_sec.map_or(Value::Null, Value::from),
        );
    }
    value
}

pub(super) async fn collect_qdrant_live_from(
    qdrant_http_url: &str,
    collection_code: &str,
    http: &reqwest::Client,
) -> Result<Value> {
    let metrics_text = http
        .get(format!("{}/metrics", qdrant_http_url))
        .send()
        .await
        .context("failed to query qdrant metrics endpoint")?
        .text()
        .await
        .context("failed to read qdrant metrics response")?;
    let metrics = parse_prometheus_sums(&metrics_text);
    let (resolved_collection_code, collection) =
        resolve_qdrant_collection_live(qdrant_http_url, collection_code, http).await?;
    let result = &collection["result"];
    Ok(json!({
        "collections_total": metric_value(&metrics, "collections_total"),
        "collections_vector_total": metric_value(&metrics, "collections_vector_total"),
        "index_optimize_queue": metric_value(&metrics, "collection_update_queue_length"),
        "running_optimizations": metric_value(&metrics, "collection_running_optimizations"),
        "update_queue_length": metric_value(&metrics, "collection_update_queue_length"),
        "memory_resident_bytes": metric_value_optional(&metrics, "memory_resident_bytes"),
        "optimizer_status": result["optimizer_status"].clone(),
        "indexed_vectors_count": result["indexed_vectors_count"].clone(),
        "points_count": result["points_count"].clone(),
        "segments_count": result["segments_count"].clone(),
        "effective_collection_code": resolved_collection_code,
    }))
}

async fn resolve_qdrant_collection_live(
    qdrant_http_url: &str,
    collection_code: &str,
    http: &reqwest::Client,
) -> Result<(String, Value)> {
    if let Some(collection) =
        fetch_qdrant_collection_json(qdrant_http_url, collection_code, http).await?
    {
        return Ok((collection_code.to_string(), collection));
    }
    let Some(discovered_collection_code) =
        discover_single_qdrant_collection_code(qdrant_http_url, http).await?
    else {
        bail!(
            "qdrant collection {} is unavailable and no single fallback collection could be discovered",
            collection_code
        );
    };
    let discovered_collection =
        fetch_qdrant_collection_json(qdrant_http_url, &discovered_collection_code, http)
            .await?
            .ok_or_else(|| {
                anyhow!(
                    "qdrant fallback collection {} disappeared before it could be queried",
                    discovered_collection_code
                )
            })?;
    Ok((discovered_collection_code, discovered_collection))
}

async fn fetch_qdrant_collection_json(
    qdrant_http_url: &str,
    collection_code: &str,
    http: &reqwest::Client,
) -> Result<Option<Value>> {
    let response = http
        .get(format!(
            "{}/collections/{}",
            qdrant_http_url, collection_code
        ))
        .send()
        .await
        .context("failed to query qdrant collection endpoint")?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !response.status().is_success() {
        bail!(
            "qdrant collection endpoint {} returned HTTP {}",
            collection_code,
            response.status()
        );
    }
    let collection = response
        .json()
        .await
        .context("failed to decode qdrant collection response")?;
    Ok(Some(collection))
}

async fn discover_single_qdrant_collection_code(
    qdrant_http_url: &str,
    http: &reqwest::Client,
) -> Result<Option<String>> {
    let response = http
        .get(format!("{}/collections", qdrant_http_url))
        .send()
        .await
        .context("failed to query qdrant collections endpoint")?;
    if !response.status().is_success() {
        bail!(
            "qdrant collections endpoint returned HTTP {}",
            response.status()
        );
    }
    let collections: Value = response
        .json()
        .await
        .context("failed to decode qdrant collections response")?;
    let mut names = collections["result"]["collections"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item["name"].as_str())
        .map(str::to_string)
        .collect::<Vec<_>>();
    names.sort();
    names.dedup();
    if names.len() == 1 {
        Ok(names.into_iter().next())
    } else {
        Ok(None)
    }
}

pub(super) async fn collect_qdrant_live(cfg: &AppConfig, http: &reqwest::Client) -> Result<Value> {
    collect_qdrant_live_from(&cfg.qdrant_http_url, &cfg.qdrant_collection_code, http).await
}

pub(super) async fn collect_optional_benchmark_qdrant_live(
    cfg: &AppConfig,
    http: &reqwest::Client,
) -> Value {
    let Some(qdrant_http_url) = cfg.benchmark_qdrant_http_url.as_deref() else {
        return json!({
            "available": false,
            "configured": false,
            "reason": "missing benchmark qdrant config",
        });
    };
    let Some(collection_code) = cfg.benchmark_qdrant_collection_code.as_deref() else {
        return json!({
            "available": false,
            "configured": false,
            "reason": "missing benchmark qdrant collection config",
        });
    };
    let benchmark_active = discover_repo_root(None)
        .ok()
        .and_then(|repo_root| {
            external_benchmark::benchmark_run_active_for_qdrant_http_url(
                &repo_root,
                qdrant_http_url,
            )
        })
        .unwrap_or(false);
    let benchmark_run_summary = discover_repo_root(None).ok().and_then(|repo_root| {
        external_benchmark::benchmark_run_summary_for_qdrant_http_url(&repo_root, qdrant_http_url)
    });
    match collect_qdrant_live_from(qdrant_http_url, collection_code, http).await {
        Ok(mut value) => {
            if let Some(object) = value.as_object_mut() {
                object.insert("available".to_string(), Value::Bool(true));
                object.insert("configured".to_string(), Value::Bool(true));
                object.insert("active".to_string(), Value::Bool(benchmark_active));
                object.insert("from_last_success".to_string(), Value::Bool(false));
                object.insert(
                    "http_url".to_string(),
                    Value::String(qdrant_http_url.to_string()),
                );
                object.insert(
                    "collection_code".to_string(),
                    Value::String(collection_code.to_string()),
                );
                object.insert(
                    "captured_at_epoch_ms".to_string(),
                    Value::from(now_epoch_ms()),
                );
                if let Some(run_summary) = benchmark_run_summary.clone() {
                    let mut run_summary = run_summary;
                    if let Ok(repo_root) = discover_repo_root(None) {
                        external_benchmark::enrich_untracked_ann_run_summary(
                            &repo_root,
                            &mut run_summary,
                        );
                    }
                    object.insert("run_summary".to_string(), run_summary);
                }
            }
            persist_last_successful_benchmark_qdrant_snapshot(&value);
            value
        }
        Err(_error) => load_last_successful_benchmark_qdrant_snapshot()
            .map(|mut cached| {
                if let Some(object) = cached.as_object_mut() {
                    object.insert("available".to_string(), Value::Bool(false));
                    object.insert("configured".to_string(), Value::Bool(true));
                    object.insert("active".to_string(), Value::Bool(false));
                    object.insert("from_last_success".to_string(), Value::Bool(true));
                    object.insert(
                        "http_url".to_string(),
                        Value::String(qdrant_http_url.to_string()),
                    );
                    object.insert(
                        "collection_code".to_string(),
                        Value::String(collection_code.to_string()),
                    );
                    if let Some(run_summary) = benchmark_run_summary.clone() {
                        let mut run_summary = run_summary;
                        if let Ok(repo_root) = discover_repo_root(None) {
                            external_benchmark::enrich_untracked_ann_run_summary(
                                &repo_root,
                                &mut run_summary,
                            );
                        }
                        object.insert("run_summary".to_string(), run_summary);
                    }
                }
                cached
            })
            .unwrap_or_else(|| {
                json!({
                    "available": false,
                    "configured": true,
                    "active": false,
                    "from_last_success": false,
                    "http_url": qdrant_http_url,
                    "collection_code": collection_code,
                    "run_summary": benchmark_run_summary,
                })
            }),
    }
}

fn benchmark_qdrant_cache_path() -> Option<PathBuf> {
    let repo_root = discover_repo_root(None).ok()?;
    Some(
        repo_root
            .join("state")
            .join("observe")
            .join("benchmark_qdrant_last_success.json"),
    )
}

fn cold_benchmark_live_progress_path(repo_root: &Path) -> PathBuf {
    repo_root
        .join("state")
        .join("cold-benchmark")
        .join("live_progress.json")
}

pub(super) fn read_live_cold_benchmark_progress(repo_root: &Path) -> Option<Value> {
    let path = cold_benchmark_live_progress_path(repo_root);
    let raw = fs::read_to_string(&path).ok()?;
    let payload: Value = serde_json::from_str(&raw).ok()?;
    let progress = &payload["cold_benchmark_progress"];
    if progress["state"].as_str() != Some("running") {
        let _ = fs::remove_file(path);
        return None;
    }
    let pid = progress["pid"].as_u64()? as u32;
    if !cold_benchmark_pid_is_live(pid) {
        let _ = fs::remove_file(path);
        return None;
    }
    Some(payload)
}

pub(super) async fn enrich_live_cold_benchmark_progress(
    db: &Client,
    progress: Option<Value>,
) -> Result<Option<Value>> {
    let Some(mut payload) = progress else {
        return Ok(None);
    };
    let current_repo_code = payload["cold_benchmark_progress"]["current_repo_code"]
        .as_str()
        .map(str::to_string);
    if let Some(project_code) = current_repo_code {
        let indexed_files = postgres::count_documents_for_project_namespace_codes(
            db,
            &project_code,
            "cold_benchmark",
        )
        .await?;
        if let Some(progress_object) =
            payload["cold_benchmark_progress"]["progress"].as_object_mut()
        {
            progress_object.insert(
                "current_repo_indexed_files".to_string(),
                Value::from(indexed_files),
            );
        }
    }
    Ok(Some(payload))
}

#[cfg(target_os = "linux")]
fn cold_benchmark_pid_is_live(pid: u32) -> bool {
    let proc_dir = PathBuf::from("/proc").join(pid.to_string());
    if !proc_dir.exists() {
        return false;
    }
    let cmdline = fs::read(proc_dir.join("cmdline")).ok();
    cmdline
        .map(|bytes| String::from_utf8_lossy(&bytes).contains("cold-path"))
        .unwrap_or(true)
}

#[cfg(not(target_os = "linux"))]
fn cold_benchmark_pid_is_live(_pid: u32) -> bool {
    true
}

fn persist_last_successful_benchmark_qdrant_snapshot(value: &Value) {
    let Some(path) = benchmark_qdrant_cache_path() else {
        return;
    };
    let Some(parent) = path.parent() else {
        return;
    };
    if fs::create_dir_all(parent).is_err() {
        return;
    }
    let Ok(text) = serde_json::to_string_pretty(value) else {
        return;
    };
    let _ = fs::write(path, text);
}

fn load_last_successful_benchmark_qdrant_snapshot() -> Option<Value> {
    let path = benchmark_qdrant_cache_path()?;
    let raw = fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

pub(super) async fn collect_nats_live(
    cfg: &AppConfig,
    http: &reqwest::Client,
    profile: &SnapshotProfile,
) -> Result<Value> {
    let varz: Value = http
        .get(format!("{}/varz", cfg.nats_http_url))
        .send()
        .await
        .context("failed to query nats /varz")?
        .json()
        .await
        .context("failed to decode nats /varz")?;
    let jsz: Value = http
        .get(format!("{}/jsz?streams=1&consumers=1", cfg.nats_http_url))
        .send()
        .await
        .context("failed to query nats /jsz")?
        .json()
        .await
        .context("failed to decode nats /jsz")?;

    let client = nats::connect(cfg).await?;
    let mut publish_samples = Vec::with_capacity(profile.nats_publish_probe_iterations);
    for index in 0..profile.nats_publish_probe_iterations {
        let started = Instant::now();
        client
            .publish(
                "ami.event.observe.probe",
                format!("probe-{index}").into_bytes().into(),
            )
            .await
            .context("failed to publish nats probe message")?;
        client
            .flush()
            .await
            .context("failed to flush nats probe publish")?;
        publish_samples.push(started.elapsed().as_secs_f64() * 1000.0);
    }

    let jetstream_storage = jsz["storage"].as_f64().unwrap_or(0.0);
    let jetstream_max_storage = jsz["config"]["max_storage"].as_f64().unwrap_or(0.0);
    Ok(json!({
        "version": varz["version"].clone(),
        "connections": varz["connections"].clone(),
        "slow_consumers": varz["slow_consumers"].clone(),
        "in_msgs": varz["in_msgs"].clone(),
        "out_msgs": varz["out_msgs"].clone(),
        "jetstream_storage_bytes": jetstream_storage,
        "jetstream_max_storage_bytes": jetstream_max_storage,
        "jetstream_disk_usage_ratio": ratio_f64(jetstream_storage, jetstream_max_storage),
        "consumer_lag_msgs": extract_nats_consumer_lag(&jsz),
        "publish_probe_p95_ms": percentile_f64(&publish_samples, 95),
        "publish_probe_samples_ms": publish_samples,
    }))
}

pub(super) async fn collect_s3_live(cfg: &AppConfig) -> Result<Value> {
    let client = s3::connect(cfg).await?;
    let started = Instant::now();
    let buckets = s3::status_bucket_names(&client).await?;
    let list_buckets_ms = started.elapsed().as_secs_f64() * 1000.0;
    Ok(json!({
        "bucket_count": buckets.len(),
        "context_bucket_available": buckets.iter().any(|bucket| bucket == &cfg.s3_bucket_context),
        "artifacts_bucket_available": buckets.iter().any(|bucket| bucket == &cfg.s3_bucket_artifacts),
        "transcripts_bucket_available": buckets.iter().any(|bucket| bucket == &cfg.s3_bucket_transcripts),
        "list_buckets_ms": list_buckets_ms,
    }))
}
