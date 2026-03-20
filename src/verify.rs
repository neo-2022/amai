use crate::bootstrap;
use crate::cli::{
    ContextPackArgs, VerifyAccuracyArgs, VerifyBenchmarkArgs, VerifyHostileArgs, VerifyLoadArgs,
};
use crate::compatibility;
use crate::config::AppConfig;
use crate::postgres;
use crate::retrieval;
use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::path::Path;
use std::time::{Duration, Instant};
use tokio::process::Command as ProcessCommand;
use tokio::time::sleep;
use tokio_postgres::Client;

pub async fn run_benchmark(
    cfg: &AppConfig,
    db: &mut Client,
    args: &VerifyBenchmarkArgs,
) -> Result<()> {
    if args.iterations == 0 {
        return Err(anyhow!("benchmark iterations must be greater than zero"));
    }

    for _ in 0..args.warmup {
        retrieval::execute_context_pack(cfg, db, &args.context, args.persist).await?;
    }

    let mut samples_us = Vec::with_capacity(args.iterations);
    let mut resolve_scope_samples = Vec::with_capacity(args.iterations);
    let mut cache_lookup_samples = Vec::with_capacity(args.iterations);
    let mut exact_lookup_samples = Vec::with_capacity(args.iterations);
    let mut symbol_lookup_samples = Vec::with_capacity(args.iterations);
    let mut lexical_lookup_samples = Vec::with_capacity(args.iterations);
    let mut query_embed_samples = Vec::with_capacity(args.iterations);
    let mut semantic_search_samples = Vec::with_capacity(args.iterations);
    let mut semantic_hydrate_samples = Vec::with_capacity(args.iterations);
    let mut serialize_samples = Vec::with_capacity(args.iterations);
    let mut persist_samples = Vec::with_capacity(args.iterations);
    let mut last_stats = None;
    for _ in 0..args.iterations {
        let started = Instant::now();
        let stats = retrieval::execute_context_pack(cfg, db, &args.context, args.persist).await?;
        samples_us.push(started.elapsed().as_micros());
        resolve_scope_samples.push(stats.timings.resolve_scope_ms);
        cache_lookup_samples.push(stats.timings.cache_lookup_ms);
        exact_lookup_samples.push(stats.timings.exact_lookup_ms);
        symbol_lookup_samples.push(stats.timings.symbol_lookup_ms);
        lexical_lookup_samples.push(stats.timings.lexical_lookup_ms);
        query_embed_samples.push(stats.timings.query_embed_ms);
        semantic_search_samples.push(stats.timings.semantic_search_ms);
        semantic_hydrate_samples.push(stats.timings.semantic_hydrate_ms);
        serialize_samples.push(stats.timings.serialize_ms);
        persist_samples.push(stats.timings.persist_ms);
        last_stats = Some(stats);
    }

    let last_stats = last_stats.ok_or_else(|| anyhow!("benchmark produced no samples"))?;
    let mut sorted = samples_us.clone();
    sorted.sort_unstable();
    let resolve_scope_p95_ms = sort_and_percentile(resolve_scope_samples, 95);
    let cache_lookup_p95_ms = sort_and_percentile(cache_lookup_samples, 95);
    let exact_lookup_p95_ms = sort_and_percentile(exact_lookup_samples, 95);
    let symbol_lookup_p95_ms = sort_and_percentile(symbol_lookup_samples, 95);
    let lexical_lookup_p95_ms = sort_and_percentile(lexical_lookup_samples, 95);
    let query_embed_p95_ms = sort_and_percentile(query_embed_samples, 95);
    let semantic_search_p95_ms = sort_and_percentile(semantic_search_samples, 95);
    let semantic_hydrate_p95_ms = sort_and_percentile(semantic_hydrate_samples, 95);
    let serialize_p95_ms = sort_and_percentile(serialize_samples, 95);
    let persist_p95_ms = sort_and_percentile(persist_samples, 95);

    let total_elapsed_us = samples_us.iter().sum::<u128>();
    let mean_ms = sample_us_to_ms(total_elapsed_us) / samples_us.len() as f64;
    let p50_ms = sample_us_to_ms(percentile_sample(&sorted, 50));
    let p95_ms = sample_us_to_ms(percentile_sample(&sorted, 95));
    let p99_ms = sample_us_to_ms(percentile_sample(&sorted, 99));
    let max_ms = sample_us_to_ms(
        *sorted
            .last()
            .ok_or_else(|| anyhow!("benchmark sample set is unexpectedly empty"))?,
    );
    let qps = if total_elapsed_us == 0 {
        args.iterations as f64 * 1_000_000.0
    } else {
        args.iterations as f64 * 1_000_000.0 / total_elapsed_us as f64
    };

    enforce_benchmark_thresholds(args, mean_ms, p95_ms, p99_ms, max_ms)?;

    let payload = json!({
        "benchmark": {
            "project": args.context.project,
            "namespace": args.context.namespace,
            "query": args.context.query,
            "retrieval_mode": args.context.retrieval_mode,
            "disable_cache": args.context.disable_cache,
            "persist": args.persist,
            "warmup": args.warmup,
            "iterations": args.iterations,
            "samples_us": samples_us,
            "mean_ms": mean_ms,
            "p50_ms": p50_ms,
            "p95_ms": p95_ms,
            "p99_ms": p99_ms,
            "max_ms": max_ms,
            "qps": qps,
        },
        "retrieval_counts": {
            "exact_documents": last_stats.exact_documents,
            "symbol_hits": last_stats.symbol_hits,
            "lexical_chunks": last_stats.lexical_chunks,
            "semantic_chunks": last_stats.semantic_chunks,
        },
        "retrieval_runtime": {
            "cache_hit": last_stats.cache_hit,
            "scope_signature": last_stats.scope_signature,
            "last_stage_timings_ms": {
                "resolve_scope_ms": last_stats.timings.resolve_scope_ms,
                "cache_lookup_ms": last_stats.timings.cache_lookup_ms,
                "exact_lookup_ms": last_stats.timings.exact_lookup_ms,
                "symbol_lookup_ms": last_stats.timings.symbol_lookup_ms,
                "lexical_lookup_ms": last_stats.timings.lexical_lookup_ms,
                "query_embed_ms": last_stats.timings.query_embed_ms,
                "semantic_search_ms": last_stats.timings.semantic_search_ms,
                "semantic_hydrate_ms": last_stats.timings.semantic_hydrate_ms,
                "serialize_ms": last_stats.timings.serialize_ms,
                "persist_ms": last_stats.timings.persist_ms,
            },
            "stage_p95_ms": {
                "resolve_scope_ms": resolve_scope_p95_ms,
                "cache_lookup_ms": cache_lookup_p95_ms,
                "exact_lookup_ms": exact_lookup_p95_ms,
                "symbol_lookup_ms": symbol_lookup_p95_ms,
                "lexical_lookup_ms": lexical_lookup_p95_ms,
                "query_embed_ms": query_embed_p95_ms,
                "semantic_search_ms": semantic_search_p95_ms,
                "semantic_hydrate_ms": semantic_hydrate_p95_ms,
                "serialize_ms": serialize_p95_ms,
                "persist_ms": persist_p95_ms,
            }
        },
        "context_pack_id": last_stats.context_pack_id,
    });

    let snapshot_kind = if args.context.disable_cache {
        "retrieval_benchmark_cold"
    } else {
        "retrieval_benchmark_hot"
    };
    let _ = postgres::insert_observability_snapshot(db, snapshot_kind, &payload).await?;
    println!("{}", serde_json::to_string_pretty(&payload)?);

    Ok(())
}

pub async fn run_accuracy(
    cfg: &AppConfig,
    db: &mut Client,
    args: &VerifyAccuracyArgs,
) -> Result<()> {
    let strict_pack = retrieval::execute_context_pack_capture(
        cfg,
        db,
        &ContextPackArgs {
            project: args.project.clone(),
            namespace: args.namespace.clone(),
            query: "beta_only_token".to_string(),
            retrieval_mode: Some("local_strict".to_string()),
            disable_cache: true,
            limit_documents: 8,
            limit_symbols: 8,
            limit_chunks: 8,
            limit_semantic_chunks: 8,
        },
        false,
    )
    .await?;

    let related_args = ContextPackArgs {
        project: args.project.clone(),
        namespace: args.namespace.clone(),
        query: "shared_runtime_marker".to_string(),
        retrieval_mode: Some("local_plus_related".to_string()),
        disable_cache: true,
        limit_documents: 8,
        limit_symbols: 8,
        limit_chunks: 8,
        limit_semantic_chunks: 8,
    };
    let mut related_pack =
        retrieval::execute_context_pack_capture(cfg, db, &related_args, false).await?;

    let symbol_pack = retrieval::execute_context_pack_capture(
        cfg,
        db,
        &ContextPackArgs {
            project: args.project.clone(),
            namespace: args.namespace.clone(),
            query: "alpha_runtime_summary".to_string(),
            retrieval_mode: Some("local_strict".to_string()),
            disable_cache: true,
            limit_documents: 8,
            limit_symbols: 8,
            limit_chunks: 8,
            limit_semantic_chunks: 8,
        },
        false,
    )
    .await?;

    let expected_related = HashSet::from([args.project.as_str(), args.related_project.as_str()]);
    let strict_visible = collect_visible_projects(&strict_pack.payload);
    let strict_visible_unexpected = strict_visible
        .iter()
        .filter(|project| project.as_str() != args.project)
        .count();
    let strict_hit_leaks = count_foreign_hits(&strict_pack.payload, &args.project);
    let cross_project_leakage = strict_visible_unexpected + strict_hit_leaks;

    let exact_precision = precision_ratio(
        &related_pack.payload["retrieval"]["exact_documents"],
        |item| {
            expected_project(item, &expected_related)
                && item["relative_path"].as_str() == Some("src/lib.rs")
                && item["snippet"]
                    .as_str()
                    .is_some_and(|snippet| snippet.contains("shared_runtime_marker"))
        },
    );
    let lexical_precision = precision_ratio(
        &related_pack.payload["retrieval"]["lexical_chunks"],
        |item| {
            expected_project(item, &expected_related)
                && item["relative_path"].as_str() == Some("src/lib.rs")
        },
    );
    let mut semantic_precision = precision_ratio(
        &related_pack.payload["retrieval"]["semantic_chunks"],
        |item| {
            expected_project(item, &expected_related)
                && item["relative_path"].as_str() == Some("src/lib.rs")
                && item["content"]
                    .as_str()
                    .is_some_and(|content| content.contains("shared_runtime_marker"))
        },
    );
    for _ in 0..3 {
        if semantic_precision > 0.0 {
            break;
        }
        sleep(Duration::from_millis(200)).await;
        related_pack =
            retrieval::execute_context_pack_capture(cfg, db, &related_args, false).await?;
        semantic_precision = precision_ratio(
            &related_pack.payload["retrieval"]["semantic_chunks"],
            |item| {
                expected_project(item, &expected_related)
                    && item["relative_path"].as_str() == Some("src/lib.rs")
                    && item["content"]
                        .as_str()
                        .is_some_and(|content| content.contains("shared_runtime_marker"))
            },
        );
    }
    let symbol_precision =
        precision_ratio(&symbol_pack.payload["retrieval"]["symbol_hits"], |item| {
            item["project_code"].as_str() == Some(args.project.as_str())
                && item["name"].as_str() == Some("alpha_runtime_summary")
        });
    let overall_precision =
        (exact_precision + lexical_precision + semantic_precision + symbol_precision) / 4.0;

    if cross_project_leakage != 0 {
        return Err(anyhow!(
            "accuracy verification failed: cross_project_leakage={cross_project_leakage}"
        ));
    }
    if symbol_precision < 1.0 || semantic_precision < 1.0 {
        return Err(anyhow!(
            "accuracy verification failed: symbol_precision={symbol_precision:.3}, semantic_precision={semantic_precision:.3}"
        ));
    }

    let payload = json!({
        "accuracy_verification": {
            "project": args.project,
            "related_project": args.related_project,
            "namespace": args.namespace,
            "cross_project_leakage": cross_project_leakage,
            "strict_visible_projects": strict_visible,
            "strict_visible_projects_unexpected": strict_visible_unexpected,
            "strict_hit_leaks": strict_hit_leaks,
            "exact_precision": exact_precision,
            "lexical_precision": lexical_precision,
            "semantic_precision": semantic_precision,
            "symbol_precision": symbol_precision,
            "overall_precision": overall_precision
        }
    });
    let _ = postgres::insert_observability_snapshot(db, "retrieval_accuracy", &payload).await?;
    println!("{}", serde_json::to_string_pretty(&payload)?);
    Ok(())
}

pub async fn run_load(cfg: &AppConfig, args: &VerifyLoadArgs) -> Result<()> {
    if args.workers == 0 || args.iterations_per_worker == 0 {
        return Err(anyhow!(
            "load verification requires workers > 0 and iterations_per_worker > 0"
        ));
    }

    let mut warmup_db = postgres::connect_admin(cfg).await?;
    for _ in 0..args.warmup_per_worker {
        retrieval::execute_context_pack(cfg, &mut warmup_db, &args.context, args.persist).await?;
    }

    let started = Instant::now();
    let mut handles = Vec::with_capacity(args.workers);
    for _ in 0..args.workers {
        let cfg = cfg.clone();
        let context = args.context.clone();
        let iterations = args.iterations_per_worker;
        let persist = args.persist;
        handles.push(tokio::spawn(async move {
            let mut db = postgres::connect_admin(&cfg).await?;
            let mut samples_us = Vec::with_capacity(iterations);
            let mut error_count = 0_usize;
            let mut last_stats = None;
            for _ in 0..iterations {
                let op_started = Instant::now();
                match retrieval::execute_context_pack(&cfg, &mut db, &context, persist).await {
                    Ok(stats) => {
                        samples_us.push(op_started.elapsed().as_micros());
                        last_stats = Some(stats);
                    }
                    Err(_) => {
                        error_count += 1;
                    }
                }
            }
            Result::<_, anyhow::Error>::Ok((samples_us, error_count, last_stats))
        }));
    }

    let mut all_samples = Vec::with_capacity(args.workers * args.iterations_per_worker);
    let mut total_errors = 0_usize;
    let mut last_stats = None;
    for handle in handles {
        let (samples, errors, worker_last_stats) = handle.await??;
        all_samples.extend(samples);
        total_errors += errors;
        if let Some(stats) = worker_last_stats {
            last_stats = Some(stats);
        }
    }
    let wall_clock_us = started.elapsed().as_micros();
    let success_count = all_samples.len();
    let total_attempts = args.workers * args.iterations_per_worker;
    let error_rate = total_errors as f64 / total_attempts as f64;

    if all_samples.is_empty() {
        return Err(anyhow!("load verification produced no successful samples"));
    }
    let mut sorted = all_samples.clone();
    sorted.sort_unstable();
    let total_elapsed_us = all_samples.iter().sum::<u128>();
    let mean_ms = sample_us_to_ms(total_elapsed_us) / all_samples.len() as f64;
    let p50_ms = sample_us_to_ms(percentile_sample(&sorted, 50));
    let p95_ms = sample_us_to_ms(percentile_sample(&sorted, 95));
    let p99_ms = sample_us_to_ms(percentile_sample(&sorted, 99));
    let max_ms = sample_us_to_ms(
        *sorted
            .last()
            .ok_or_else(|| anyhow!("load sample set is unexpectedly empty"))?,
    );
    let qps = if wall_clock_us == 0 {
        success_count as f64 * 1_000_000.0
    } else {
        success_count as f64 * 1_000_000.0 / wall_clock_us as f64
    };

    enforce_load_thresholds(args, p95_ms, qps, error_rate)?;

    let last_stats =
        last_stats.ok_or_else(|| anyhow!("load verification produced no final stats"))?;
    let payload = json!({
        "load_verification": {
            "project": args.context.project,
            "namespace": args.context.namespace,
            "query": args.context.query,
            "retrieval_mode": args.context.retrieval_mode,
            "disable_cache": args.context.disable_cache,
            "persist": args.persist,
            "workers": args.workers,
            "iterations_per_worker": args.iterations_per_worker,
            "warmup_per_worker": args.warmup_per_worker,
            "success_count": success_count,
            "error_count": total_errors,
            "error_rate": error_rate,
            "wall_clock_ms": sample_us_to_ms(wall_clock_us),
            "samples_us": all_samples,
            "mean_ms": mean_ms,
            "p50_ms": p50_ms,
            "p95_ms": p95_ms,
            "p99_ms": p99_ms,
            "max_ms": max_ms,
            "qps": qps,
        },
        "retrieval_counts": {
            "exact_documents": last_stats.exact_documents,
            "symbol_hits": last_stats.symbol_hits,
            "lexical_chunks": last_stats.lexical_chunks,
            "semantic_chunks": last_stats.semantic_chunks,
        },
        "retrieval_runtime": {
            "cache_hit": last_stats.cache_hit,
            "scope_signature": last_stats.scope_signature
        }
    });
    let snapshot_kind = if args.context.disable_cache {
        "retrieval_load_cold"
    } else {
        "retrieval_load_hot"
    };
    let db = postgres::connect_admin(cfg).await?;
    let _ = postgres::insert_observability_snapshot(&db, snapshot_kind, &payload).await?;
    println!("{}", serde_json::to_string_pretty(&payload)?);
    Ok(())
}

pub async fn run_hostile(cfg: &AppConfig, args: &VerifyHostileArgs) -> Result<()> {
    bootstrap::bootstrap_stack(cfg).await?;
    compatibility::assert_supported(cfg).await?;

    let scenario = args.scenario.as_str();
    let mut probes = Vec::new();

    match scenario {
        "all" => {
            probes.push(run_stack_meta_drift(cfg).await?);
            for service in ["postgres", "qdrant", "minio", "nats"] {
                probes.push(run_service_loss_probe(cfg, service).await?);
            }
        }
        "stack_meta_drift" => probes.push(run_stack_meta_drift(cfg).await?),
        "postgres" | "qdrant" | "minio" | "nats" => {
            probes.push(run_service_loss_probe(cfg, scenario).await?)
        }
        other => {
            return Err(anyhow!(
                "unsupported hostile scenario: {other}; use all|stack_meta_drift|postgres|qdrant|minio|nats"
            ));
        }
    }

    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "hostile_verification": {
                "scenario": scenario,
                "probes": probes,
            }
        }))?
    );

    Ok(())
}

async fn run_stack_meta_drift(cfg: &AppConfig) -> Result<serde_json::Value> {
    let db = postgres::connect_admin(cfg).await?;
    postgres::upsert_stack_meta(
        &db,
        "compatibility",
        &json!({
            "schema_version": -1,
            "compatibility_profile": "tampered-profile"
        }),
    )
    .await?;

    let report = compatibility::check(cfg).await?;
    if report.compatible() {
        return Err(anyhow!(
            "stack_meta drift probe failed: compatibility remained green after tampering"
        ));
    }

    bootstrap::bootstrap_stack(cfg).await?;
    compatibility::assert_supported(cfg).await?;

    Ok(json!({
        "probe": "stack_meta_drift",
        "fail_closed": true,
        "recovered": true,
    }))
}

async fn run_service_loss_probe(cfg: &AppConfig, service: &str) -> Result<serde_json::Value> {
    docker_compose(&["stop", service]).await?;

    let failed_closed = match compatibility::check(cfg).await {
        Ok(report) => !report.compatible(),
        Err(_) => true,
    };

    let restart_result = async {
        docker_compose(&["start", service]).await?;
        bootstrap::bootstrap_stack(cfg).await?;
        compatibility::assert_supported(cfg).await
    }
    .await;

    if !failed_closed {
        restart_result?;
        return Err(anyhow!(
            "service loss probe failed: compatibility path stayed green while {service} was unavailable"
        ));
    }

    restart_result?;

    Ok(json!({
        "probe": "service_loss",
        "service": service,
        "fail_closed": true,
        "recovered": true,
    }))
}

async fn docker_compose(args: &[&str]) -> Result<()> {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let output = ProcessCommand::new("docker")
        .arg("compose")
        .args(args)
        .current_dir(repo_root)
        .output()
        .await
        .with_context(|| format!("failed to run docker compose {}", args.join(" ")))?;
    if output.status.success() {
        return Ok(());
    }

    Err(anyhow!(
        "docker compose {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr).trim()
    ))
}

fn percentile_sample(sorted_samples: &[u128], percentile: usize) -> u128 {
    if sorted_samples.is_empty() {
        return 0;
    }
    let percentile = percentile.min(100);
    let rank = (percentile * sorted_samples.len()).div_ceil(100);
    let index = rank.saturating_sub(1).min(sorted_samples.len() - 1);
    sorted_samples[index]
}

fn sort_and_percentile(mut samples: Vec<u128>, percentile: usize) -> u128 {
    samples.sort_unstable();
    percentile_sample(&samples, percentile)
}

fn sample_us_to_ms(sample_us: u128) -> f64 {
    sample_us as f64 / 1000.0
}

fn enforce_benchmark_thresholds(
    args: &VerifyBenchmarkArgs,
    mean_ms: f64,
    p95_ms: f64,
    p99_ms: f64,
    max_ms: f64,
) -> Result<()> {
    let mut violations = Vec::new();
    if let Some(limit) = args.max_mean_ms
        && mean_ms > limit as f64
    {
        violations.push(format!("mean_ms={mean_ms:.3} exceeds {limit}"));
    }
    if let Some(limit) = args.max_p95_ms
        && p95_ms > limit as f64
    {
        violations.push(format!("p95_ms={p95_ms:.3} exceeds {limit}"));
    }
    if let Some(limit) = args.max_p99_ms
        && p99_ms > limit as f64
    {
        violations.push(format!("p99_ms={p99_ms:.3} exceeds {limit}"));
    }
    if let Some(limit) = args.max_max_ms
        && max_ms > limit as f64
    {
        violations.push(format!("max_ms={max_ms:.3} exceeds {limit}"));
    }

    if violations.is_empty() {
        return Ok(());
    }

    Err(anyhow!(
        "benchmark thresholds violated: {}",
        violations.join("; ")
    ))
}

fn enforce_load_thresholds(
    args: &VerifyLoadArgs,
    p95_ms: f64,
    qps: f64,
    error_rate: f64,
) -> Result<()> {
    let mut violations = Vec::new();
    if let Some(limit) = args.max_p95_ms
        && p95_ms > limit as f64
    {
        violations.push(format!("p95_ms={p95_ms:.3} exceeds {limit}"));
    }
    if let Some(limit) = args.min_qps
        && qps < limit
    {
        violations.push(format!("qps={qps:.2} below {limit:.2}"));
    }
    if let Some(limit) = args.max_error_rate
        && error_rate > limit
    {
        violations.push(format!("error_rate={error_rate:.4} exceeds {limit:.4}"));
    }

    if violations.is_empty() {
        return Ok(());
    }

    Err(anyhow!(
        "load thresholds violated: {}",
        violations.join("; ")
    ))
}

fn collect_visible_projects(payload: &Value) -> Vec<String> {
    payload["visible_projects"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item["project_code"].as_str().map(ToOwned::to_owned))
        .collect()
}

fn count_foreign_hits(payload: &Value, local_project: &str) -> usize {
    [
        &payload["retrieval"]["exact_documents"],
        &payload["retrieval"]["symbol_hits"],
        &payload["retrieval"]["lexical_chunks"],
        &payload["retrieval"]["semantic_chunks"],
    ]
    .into_iter()
    .map(|items| {
        items
            .as_array()
            .into_iter()
            .flatten()
            .filter(|item| !item_belongs_to_project(item, local_project))
            .count()
    })
    .sum()
}

fn item_belongs_to_project(item: &Value, project: &str) -> bool {
    item["project_code"].as_str() == Some(project)
        || item["provenance"]["source_project"].as_str() == Some(project)
}

fn expected_project(item: &Value, expected_projects: &HashSet<&str>) -> bool {
    item["project_code"]
        .as_str()
        .or_else(|| item["provenance"]["source_project"].as_str())
        .is_some_and(|project| expected_projects.contains(project))
}

fn precision_ratio(items: &Value, predicate: impl Fn(&Value) -> bool) -> f64 {
    let Some(items) = items.as_array() else {
        return 0.0;
    };
    if items.is_empty() {
        return 0.0;
    }
    let correct = items.iter().filter(|item| predicate(item)).count();
    correct as f64 / items.len() as f64
}

#[cfg(test)]
mod tests {
    use super::{item_belongs_to_project, percentile_sample, precision_ratio};
    use serde_json::json;

    #[test]
    fn percentile_uses_ceil_rank() {
        let samples = vec![10_u128, 20, 30, 40, 50];
        assert_eq!(percentile_sample(&samples, 50), 30);
        assert_eq!(percentile_sample(&samples, 95), 50);
    }

    #[test]
    fn percentile_handles_empty_input() {
        assert_eq!(percentile_sample(&[], 95), 0);
    }

    #[test]
    fn precision_ratio_counts_correct_hits() {
        let items = json!([
            {"project_code":"alpha","relative_path":"src/lib.rs"},
            {"project_code":"alpha","relative_path":"src/other.rs"}
        ]);
        let ratio = precision_ratio(&items, |item| {
            item["relative_path"].as_str() == Some("src/lib.rs")
        });
        assert!((ratio - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn item_belongs_to_project_checks_project_and_provenance() {
        assert!(item_belongs_to_project(
            &json!({"project_code":"alpha"}),
            "alpha"
        ));
        assert!(item_belongs_to_project(
            &json!({"provenance":{"source_project":"alpha"}}),
            "alpha"
        ));
        assert!(!item_belongs_to_project(
            &json!({"project_code":"beta","provenance":{"source_project":"beta"}}),
            "alpha"
        ));
    }
}
