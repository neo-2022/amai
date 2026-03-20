use crate::bootstrap;
use crate::cli::{VerifyBenchmarkArgs, VerifyHostileArgs};
use crate::compatibility;
use crate::config::AppConfig;
use crate::postgres;
use crate::retrieval;
use anyhow::{Context, Result, anyhow};
use serde_json::json;
use std::path::Path;
use std::time::Instant;
use tokio::process::Command as ProcessCommand;
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

    let mut samples_ms = Vec::with_capacity(args.iterations);
    let mut last_stats = None;
    for _ in 0..args.iterations {
        let started = Instant::now();
        let stats = retrieval::execute_context_pack(cfg, db, &args.context, args.persist).await?;
        samples_ms.push(started.elapsed().as_millis());
        last_stats = Some(stats);
    }

    let last_stats = last_stats.ok_or_else(|| anyhow!("benchmark produced no samples"))?;
    let mut sorted = samples_ms.clone();
    sorted.sort_unstable();

    let mean_ms = samples_ms.iter().sum::<u128>() as f64 / samples_ms.len() as f64;
    let p50_ms = percentile_ms(&sorted, 50);
    let p95_ms = percentile_ms(&sorted, 95);
    let max_ms = *sorted
        .last()
        .ok_or_else(|| anyhow!("benchmark sample set is unexpectedly empty"))?;

    enforce_benchmark_thresholds(args, mean_ms, p95_ms, max_ms)?;

    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "benchmark": {
                "project": args.context.project,
                "namespace": args.context.namespace,
                "query": args.context.query,
                "retrieval_mode": args.context.retrieval_mode,
                "persist": args.persist,
                "warmup": args.warmup,
                "iterations": args.iterations,
                "samples_ms": samples_ms,
                "mean_ms": mean_ms,
                "p50_ms": p50_ms,
                "p95_ms": p95_ms,
                "max_ms": max_ms,
            },
            "retrieval_counts": {
                "exact_documents": last_stats.exact_documents,
                "symbol_hits": last_stats.symbol_hits,
                "lexical_chunks": last_stats.lexical_chunks,
                "semantic_chunks": last_stats.semantic_chunks,
            },
            "context_pack_id": last_stats.context_pack_id,
        }))?
    );

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

fn percentile_ms(sorted_samples: &[u128], percentile: usize) -> u128 {
    if sorted_samples.is_empty() {
        return 0;
    }
    let percentile = percentile.min(100);
    let rank = (percentile * sorted_samples.len()).div_ceil(100);
    let index = rank.saturating_sub(1).min(sorted_samples.len() - 1);
    sorted_samples[index]
}

fn enforce_benchmark_thresholds(
    args: &VerifyBenchmarkArgs,
    mean_ms: f64,
    p95_ms: u128,
    max_ms: u128,
) -> Result<()> {
    let mut violations = Vec::new();
    if let Some(limit) = args.max_mean_ms
        && mean_ms > limit as f64
    {
        violations.push(format!("mean_ms={mean_ms:.2} exceeds {limit}"));
    }
    if let Some(limit) = args.max_p95_ms
        && p95_ms > limit
    {
        violations.push(format!("p95_ms={p95_ms} exceeds {limit}"));
    }
    if let Some(limit) = args.max_max_ms
        && max_ms > limit
    {
        violations.push(format!("max_ms={max_ms} exceeds {limit}"));
    }

    if violations.is_empty() {
        return Ok(());
    }

    Err(anyhow!(
        "benchmark thresholds violated: {}",
        violations.join("; ")
    ))
}

#[cfg(test)]
mod tests {
    use super::percentile_ms;

    #[test]
    fn percentile_uses_ceil_rank() {
        let samples = vec![10_u128, 20, 30, 40, 50];
        assert_eq!(percentile_ms(&samples, 50), 30);
        assert_eq!(percentile_ms(&samples, 95), 50);
    }

    #[test]
    fn percentile_handles_empty_input() {
        assert_eq!(percentile_ms(&[], 95), 0);
    }
}
