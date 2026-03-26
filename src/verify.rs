use crate::bootstrap;
use crate::cli::{
    ContextPackArgs, VerifyAccuracyArgs, VerifyBenchmarkArgs, VerifyDegradationArgs,
    VerifyHostileArgs, VerifyLoadArgs, VerifyTextCompareArgs, VerifyTokenBenchmarkArgs,
    VerifyTokenBenchmarkSuiteArgs,
};
use crate::compatibility;
use crate::config::AppConfig;
use crate::degradation_proof;
use crate::eval_verdict::{self, EvalPattern, EvalSignals};
use crate::language;
use crate::postgres;
use crate::retrieval;
use crate::retrieval_science;
use crate::token_budget;
use anyhow::{Context, Result, anyhow};
use ignore::WalkBuilder;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tiktoken_rs::{CoreBPE, cl100k_base, o200k_base};
use tokio::process::Command as ProcessCommand;
use tokio::time::sleep;
use tokio_postgres::Client;
use uuid::Uuid;

#[derive(Debug)]
struct LoadWorkerOutcome {
    worker_index: usize,
    samples_ns: Vec<u128>,
    error_count: usize,
    cache_probe_miss_count: usize,
    last_stats: Option<retrieval::ContextPackStats>,
}

#[derive(Debug, Clone, Deserialize)]
struct AccuracySuiteFile {
    suite: AccuracySuiteManifest,
}

#[derive(Debug, Clone, Deserialize)]
struct AccuracySuiteManifest {
    suite_version: String,
    dataset_version: String,
    query_suite_version: String,
    scoring_rules_version: String,
    summary: String,
    strict_query: String,
    related_query: String,
    namespace_query: String,
    hostile_mixed_query: String,
    strict_namespace: String,
    related_path: String,
    related_term: String,
    symbol_name: String,
}

struct AccuracyEvalProbe {
    name: &'static str,
    verdict_class: String,
    verdict_reason: String,
    details: Value,
}

fn normalize_engineering_context(
    context: &ContextPackArgs,
    engineering_default_source_kind: &str,
) -> ContextPackArgs {
    let mut normalized = context.clone();
    if normalized.token_source_kind.trim().is_empty()
        || normalized.token_source_kind == "live_context_pack"
    {
        normalized.token_source_kind = engineering_default_source_kind.to_string();
    }
    normalized
}

pub async fn run_benchmark(
    cfg: &AppConfig,
    db: &mut Client,
    args: &VerifyBenchmarkArgs,
) -> Result<()> {
    let context = normalize_engineering_context(&args.context, "benchmark_context_pack");
    if args.iterations == 0 {
        return Err(anyhow!("benchmark iterations must be greater than zero"));
    }

    for _ in 0..args.warmup {
        retrieval::execute_context_pack_with_options(cfg, db, &context, args.persist, false)
            .await?;
    }

    let mut samples_ns = Vec::with_capacity(args.iterations);
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
        let stats =
            retrieval::execute_context_pack_with_options(cfg, db, &context, args.persist, false)
                .await?;
        samples_ns.push(started.elapsed().as_nanos());
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
    let mut sorted = samples_ns.clone();
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

    let total_elapsed_ns = samples_ns.iter().sum::<u128>();
    let mean_ms = sample_ns_to_ms(total_elapsed_ns) / samples_ns.len() as f64;
    let p50_ms = sample_ns_to_ms(percentile_sample(&sorted, 50));
    let p95_ms = sample_ns_to_ms(percentile_sample(&sorted, 95));
    let p99_ms = sample_ns_to_ms(percentile_sample(&sorted, 99));
    let max_ms = sample_ns_to_ms(
        *sorted
            .last()
            .ok_or_else(|| anyhow!("benchmark sample set is unexpectedly empty"))?,
    );
    let qps = if total_elapsed_ns == 0 {
        args.iterations as f64 * 1_000_000_000.0
    } else {
        args.iterations as f64 * 1_000_000_000.0 / total_elapsed_ns as f64
    };
    let benchmark_run_id = Uuid::new_v4();
    let captured_at_epoch_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock before unix epoch")?
        .as_millis() as u64;

    enforce_benchmark_thresholds(args, mean_ms, p95_ms, p99_ms, max_ms)?;
    let suite_key = if context.disable_cache {
        "retrieval_benchmark_cold"
    } else {
        "retrieval_benchmark_hot"
    };

    let payload = json!({
        "_observability": {
            "source_event_id": benchmark_run_id,
            "source_kind": "benchmark_run",
            "scope_project_code": context.project,
            "scope_namespace_code": context.namespace,
            "captured_at_epoch_ms": captured_at_epoch_ms
        },
        "benchmark": {
            "project": context.project,
            "namespace": context.namespace,
            "query": context.query,
            "retrieval_mode": context.retrieval_mode,
            "disable_cache": context.disable_cache,
            "persist": args.persist,
            "warmup": args.warmup,
            "iterations": args.iterations,
            "sample_resolution": "ns",
            "samples_ns": samples_ns,
            "samples_us": samples_ns
                .iter()
                .map(|sample_ns| sample_ns / 1000)
                .collect::<Vec<_>>(),
            "mean_ms": mean_ms,
            "p50_ms": p50_ms,
            "p95_ms": p95_ms,
            "p99_ms": p99_ms,
            "max_ms": max_ms,
            "qps": qps,
            "benchmark_run_id": benchmark_run_id,
            "captured_at_epoch_ms": captured_at_epoch_ms,
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
        "retrieval_science": retrieval_science::suite_metadata(suite_key)?,
        "degradation_policy": retrieval_science::degradation_policy_json()?,
        "context_pack_id": last_stats.context_pack_id,
    });

    let snapshot_kind = if context.disable_cache {
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
    let suite = load_accuracy_suite(&args.manifest)?;
    let strict_pack = retrieval::execute_context_pack_capture_with_options(
        cfg,
        db,
        &ContextPackArgs {
            project: args.project.clone(),
            namespace: args.namespace.clone(),
            query: suite.strict_query.clone(),
            retrieval_mode: Some("local_strict".to_string()),
            disable_cache: true,
            limit_documents: 8,
            limit_symbols: 8,
            limit_chunks: 8,
            limit_semantic_chunks: 8,
            token_source_kind: "verify_context_pack".to_string(),
            client_prompt_tokens: None,
            assistant_generation_tokens: None,
            tool_overhead_tokens: None,
            continuity_restore_tokens: None,
        },
        false,
        false,
    )
    .await?;

    let related_args = ContextPackArgs {
        project: args.project.clone(),
        namespace: args.namespace.clone(),
        query: suite.related_query.clone(),
        retrieval_mode: Some("local_plus_related".to_string()),
        disable_cache: true,
        limit_documents: 8,
        limit_symbols: 8,
        limit_chunks: 8,
        limit_semantic_chunks: 8,
        token_source_kind: "verify_context_pack".to_string(),
        client_prompt_tokens: None,
        assistant_generation_tokens: None,
        tool_overhead_tokens: None,
        continuity_restore_tokens: None,
    };
    let mut related_pack =
        retrieval::execute_context_pack_capture_with_options(cfg, db, &related_args, false, false)
            .await?;

    let symbol_pack = retrieval::execute_context_pack_capture_with_options(
        cfg,
        db,
        &ContextPackArgs {
            project: args.project.clone(),
            namespace: args.namespace.clone(),
            query: suite.namespace_query.clone(),
            retrieval_mode: Some("local_strict".to_string()),
            disable_cache: true,
            limit_documents: 8,
            limit_symbols: 8,
            limit_chunks: 8,
            limit_semantic_chunks: 8,
            token_source_kind: "verify_context_pack".to_string(),
            client_prompt_tokens: None,
            assistant_generation_tokens: None,
            tool_overhead_tokens: None,
            continuity_restore_tokens: None,
        },
        false,
        false,
    )
    .await?;

    let namespace_strict_pack = retrieval::execute_context_pack_capture_with_options(
        cfg,
        db,
        &ContextPackArgs {
            project: args.project.clone(),
            namespace: suite.strict_namespace.clone(),
            query: suite.namespace_query.clone(),
            retrieval_mode: Some("local_strict".to_string()),
            disable_cache: true,
            limit_documents: 8,
            limit_symbols: 8,
            limit_chunks: 8,
            limit_semantic_chunks: 8,
            token_source_kind: "verify_context_pack".to_string(),
            client_prompt_tokens: None,
            assistant_generation_tokens: None,
            tool_overhead_tokens: None,
            continuity_restore_tokens: None,
        },
        false,
        false,
    )
    .await?;
    let hostile_pack = retrieval::execute_context_pack_capture_with_options(
        cfg,
        db,
        &ContextPackArgs {
            project: args.project.clone(),
            namespace: args.namespace.clone(),
            query: suite.hostile_mixed_query.clone(),
            retrieval_mode: Some("local_strict".to_string()),
            disable_cache: true,
            limit_documents: 8,
            limit_symbols: 8,
            limit_chunks: 8,
            limit_semantic_chunks: 8,
            token_source_kind: "verify_context_pack".to_string(),
            client_prompt_tokens: None,
            assistant_generation_tokens: None,
            tool_overhead_tokens: None,
            continuity_restore_tokens: None,
        },
        false,
        false,
    )
    .await?;

    let expected_related = HashSet::from([args.project.as_str(), args.related_project.as_str()]);
    let strict_visible = collect_visible_projects(&strict_pack.payload);
    let strict_visible_unexpected = strict_visible
        .iter()
        .filter(|project| project.as_str() != args.project)
        .count();
    let strict_visible_namespaces = collect_visible_namespaces(&strict_pack.payload);
    let strict_visible_namespaces_unexpected = strict_visible_namespaces
        .iter()
        .filter(|namespace| namespace.as_str() != args.namespace)
        .count();
    let strict_hit_leaks = count_foreign_hits(&strict_pack.payload, &args.project);
    let strict_namespace_hit_leaks =
        count_foreign_namespace_hits(&strict_pack.payload, &args.namespace);
    let cross_project_leakage = strict_visible_unexpected + strict_hit_leaks;
    let strict_cross_namespace_leakage =
        strict_visible_namespaces_unexpected + strict_namespace_hit_leaks;
    let namespace_visible = collect_visible_namespaces(&namespace_strict_pack.payload);
    let namespace_visible_projects = collect_visible_projects(&namespace_strict_pack.payload);
    let namespace_visible_projects_unexpected = namespace_visible_projects
        .iter()
        .filter(|project| project.as_str() != args.project)
        .count();
    let namespace_visible_unexpected = namespace_visible
        .iter()
        .filter(|namespace| namespace.as_str() != suite.strict_namespace.as_str())
        .count();
    let namespace_hit_leaks =
        count_foreign_namespace_hits(&namespace_strict_pack.payload, &suite.strict_namespace);
    let cross_namespace_leakage = namespace_visible_unexpected + namespace_hit_leaks;
    let hostile_visible = collect_visible_projects(&hostile_pack.payload);
    let hostile_visible_unexpected = hostile_visible
        .iter()
        .filter(|project| project.as_str() != args.project)
        .count();
    let hostile_visible_namespaces = collect_visible_namespaces(&hostile_pack.payload);
    let hostile_visible_namespaces_unexpected = hostile_visible_namespaces
        .iter()
        .filter(|namespace| namespace.as_str() != args.namespace)
        .count();
    let hostile_hit_leaks = count_foreign_hits(&hostile_pack.payload, &args.project);
    let hostile_namespace_hit_leaks =
        count_foreign_namespace_hits(&hostile_pack.payload, &args.namespace);
    let hostile_cross_project_leakage = hostile_visible_unexpected + hostile_hit_leaks;
    let hostile_cross_namespace_leakage =
        hostile_visible_namespaces_unexpected + hostile_namespace_hit_leaks;

    let exact_precision = precision_ratio(
        &related_pack.payload["retrieval"]["exact_documents"],
        |item| {
            expected_project(item, &expected_related)
                && item["relative_path"].as_str() == Some(suite.related_path.as_str())
                && item["snippet"]
                    .as_str()
                    .is_some_and(|snippet| snippet.contains(suite.related_term.as_str()))
        },
    );
    let lexical_precision = precision_ratio(
        &related_pack.payload["retrieval"]["lexical_chunks"],
        |item| {
            expected_project(item, &expected_related)
                && item["relative_path"].as_str() == Some(suite.related_path.as_str())
        },
    );
    let mut semantic_precision = precision_ratio(
        &related_pack.payload["retrieval"]["semantic_chunks"],
        |item| {
            expected_project(item, &expected_related)
                && item["relative_path"].as_str() == Some(suite.related_path.as_str())
                && item["content"]
                    .as_str()
                    .is_some_and(|content| content.contains(suite.related_term.as_str()))
        },
    );
    for _ in 0..3 {
        if semantic_precision > 0.0 {
            break;
        }
        sleep(Duration::from_millis(200)).await;
        related_pack = retrieval::execute_context_pack_capture_with_options(
            cfg,
            db,
            &related_args,
            false,
            false,
        )
        .await?;
        semantic_precision = precision_ratio(
            &related_pack.payload["retrieval"]["semantic_chunks"],
            |item| {
                expected_project(item, &expected_related)
                    && item["relative_path"].as_str() == Some(suite.related_path.as_str())
                    && item["content"]
                        .as_str()
                        .is_some_and(|content| content.contains(suite.related_term.as_str()))
            },
        );
    }
    let symbol_precision =
        precision_ratio(&symbol_pack.payload["retrieval"]["symbol_hits"], |item| {
            item["project_code"].as_str() == Some(args.project.as_str())
                && item["name"].as_str() == Some(suite.symbol_name.as_str())
        });
    let overall_precision =
        (exact_precision + lexical_precision + semantic_precision + symbol_precision) / 4.0;
    let formal_invariants = vec![
        json!({
            "name": "strict_local_visible_projects_only",
            "expected": args.project,
            "observed": strict_visible,
            "pass": strict_visible_unexpected == 0,
        }),
        json!({
            "name": "strict_local_hits_do_not_leak_projects",
            "expected": 0,
            "observed": strict_hit_leaks,
            "pass": strict_hit_leaks == 0,
        }),
        json!({
            "name": "hostile_mixed_query_fail_closed",
            "expected": 0,
            "observed": hostile_cross_project_leakage,
            "pass": hostile_cross_project_leakage == 0,
        }),
        json!({
            "name": "hostile_mixed_query_visible_projects_only",
            "expected": args.project,
            "observed": hostile_visible,
            "pass": hostile_visible_unexpected == 0,
        }),
        json!({
            "name": "hostile_mixed_query_hits_do_not_leak_projects",
            "expected": 0,
            "observed": hostile_hit_leaks,
            "pass": hostile_hit_leaks == 0,
        }),
        json!({
            "name": "strict_local_visible_namespaces_only",
            "expected": args.namespace,
            "observed": strict_visible_namespaces,
            "pass": strict_visible_namespaces_unexpected == 0,
        }),
        json!({
            "name": "strict_local_hits_do_not_leak_namespaces",
            "expected": 0,
            "observed": strict_namespace_hit_leaks,
            "pass": strict_namespace_hit_leaks == 0,
        }),
        json!({
            "name": "hostile_mixed_query_visible_namespaces_only",
            "expected": args.namespace,
            "observed": hostile_visible_namespaces,
            "pass": hostile_visible_namespaces_unexpected == 0,
        }),
        json!({
            "name": "hostile_mixed_query_hits_do_not_leak_namespaces",
            "expected": 0,
            "observed": hostile_namespace_hit_leaks,
            "pass": hostile_namespace_hit_leaks == 0,
        }),
        json!({
            "name": "namespace_strict_visible_projects_only",
            "expected": args.project,
            "observed": namespace_visible_projects,
            "pass": namespace_visible_projects_unexpected == 0,
        }),
        json!({
            "name": "namespace_strict_hits_do_not_leak_namespaces",
            "expected": 0,
            "observed": namespace_hit_leaks,
            "pass": namespace_hit_leaks == 0,
        }),
        json!({
            "name": "namespace_strict_fail_closed",
            "expected": 0,
            "observed": cross_namespace_leakage,
            "pass": cross_namespace_leakage == 0,
        }),
        json!({
            "name": "symbol_precision_exact",
            "expected": 1.0,
            "observed": symbol_precision,
            "pass": (symbol_precision - 1.0).abs() < f64::EPSILON,
        }),
        json!({
            "name": "semantic_precision_exact",
            "expected": 1.0,
            "observed": semantic_precision,
            "pass": (semantic_precision - 1.0).abs() < f64::EPSILON,
        }),
    ];
    let eval_probes = build_accuracy_eval_probes(
        args,
        &suite,
        &symbol_pack,
        &namespace_strict_pack,
        exact_precision,
        lexical_precision,
        semantic_precision,
        cross_project_leakage,
        strict_cross_namespace_leakage,
        hostile_cross_project_leakage,
        hostile_cross_namespace_leakage,
        cross_namespace_leakage,
        namespace_visible_projects_unexpected,
    )?;
    let canonical_eval = build_accuracy_canonical_eval(&eval_probes)?;

    if cross_project_leakage != 0 {
        return Err(anyhow!(
            "accuracy verification failed: cross_project_leakage={cross_project_leakage}"
        ));
    }
    if hostile_cross_project_leakage != 0 {
        return Err(anyhow!(
            "accuracy verification failed: hostile_cross_project_leakage={hostile_cross_project_leakage}"
        ));
    }
    if strict_cross_namespace_leakage != 0 {
        return Err(anyhow!(
            "accuracy verification failed: strict_cross_namespace_leakage={strict_cross_namespace_leakage}"
        ));
    }
    if hostile_cross_namespace_leakage != 0 {
        return Err(anyhow!(
            "accuracy verification failed: hostile_cross_namespace_leakage={hostile_cross_namespace_leakage}"
        ));
    }
    if cross_namespace_leakage != 0 {
        return Err(anyhow!(
            "accuracy verification failed: cross_namespace_leakage={cross_namespace_leakage}"
        ));
    }
    if symbol_precision < 1.0 || semantic_precision < 1.0 {
        return Err(anyhow!(
            "accuracy verification failed: symbol_precision={symbol_precision:.3}, semantic_precision={semantic_precision:.3}"
        ));
    }

    let accuracy_run_id = Uuid::new_v4();
    let captured_at_epoch_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock before unix epoch")?
        .as_millis() as u64;
    let payload = json!({
        "_observability": {
            "source_event_id": accuracy_run_id,
            "source_kind": "accuracy_verification_run",
            "scope_project_code": args.project,
            "scope_namespace_code": args.namespace,
            "captured_at_epoch_ms": captured_at_epoch_ms
        },
        "accuracy_verification": {
            "accuracy_run_id": accuracy_run_id,
            "captured_at_epoch_ms": captured_at_epoch_ms,
            "project": args.project,
            "related_project": args.related_project,
            "namespace": args.namespace,
            "suite_version": suite.suite_version,
            "dataset_version": suite.dataset_version,
            "query_suite_version": suite.query_suite_version,
            "scoring_rules_version": suite.scoring_rules_version,
            "suite_summary": suite.summary,
            "cross_project_leakage": cross_project_leakage,
            "strict_visible_projects": strict_visible,
            "strict_visible_projects_unexpected": strict_visible_unexpected,
            "strict_hit_leaks": strict_hit_leaks,
            "strict_visible_namespaces": strict_visible_namespaces,
            "strict_visible_namespaces_unexpected": strict_visible_namespaces_unexpected,
            "strict_namespace_hit_leaks": strict_namespace_hit_leaks,
            "strict_cross_namespace_leakage": strict_cross_namespace_leakage,
            "hostile_mixed_query": suite.hostile_mixed_query,
            "hostile_visible_projects": hostile_visible,
            "hostile_visible_projects_unexpected": hostile_visible_unexpected,
            "hostile_hit_leaks": hostile_hit_leaks,
            "hostile_cross_project_leakage": hostile_cross_project_leakage,
            "hostile_visible_namespaces": hostile_visible_namespaces,
            "hostile_visible_namespaces_unexpected": hostile_visible_namespaces_unexpected,
            "hostile_namespace_hit_leaks": hostile_namespace_hit_leaks,
            "hostile_cross_namespace_leakage": hostile_cross_namespace_leakage,
            "cross_namespace_leakage": cross_namespace_leakage,
            "namespace_strict_visible_projects": namespace_visible_projects,
            "namespace_strict_visible_projects_unexpected": namespace_visible_projects_unexpected,
            "namespace_strict_visible_namespaces": namespace_visible,
            "namespace_strict_visible_namespaces_unexpected": namespace_visible_unexpected,
            "namespace_strict_hit_leaks": namespace_hit_leaks,
            "exact_precision": exact_precision,
            "lexical_precision": lexical_precision,
            "semantic_precision": semantic_precision,
            "symbol_precision": symbol_precision,
            "overall_precision": overall_precision,
            "formal_invariants": formal_invariants,
            "canonical_eval": canonical_eval
        },
        "retrieval_science": retrieval_science::suite_metadata("retrieval_accuracy")?,
        "degradation_policy": retrieval_science::degradation_policy_json()?
    });
    let _ = postgres::insert_observability_snapshot(db, "retrieval_accuracy", &payload).await?;
    println!("{}", serde_json::to_string_pretty(&payload)?);
    Ok(())
}

pub async fn run_degradation(
    cfg: &AppConfig,
    db: &mut Client,
    args: &VerifyDegradationArgs,
) -> Result<()> {
    let scenario = args.scenario.as_str();
    if scenario != "all" {
        return Err(anyhow!(
            "unsupported degradation scenario: {scenario}; use all"
        ));
    }

    let captured_at_epoch_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock before unix epoch")?
        .as_millis() as u64;
    let verification_run_id = Uuid::new_v4();
    let proof = degradation_proof::build_report(captured_at_epoch_ms, cfg.local_fast_cache_ttl_ms)?;
    let scenarios = proof["degradation_verification"]["scenarios"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let pass = scenarios
        .iter()
        .filter(|item| item["status"].as_str() == Some("pass"))
        .count() as u64;
    let critical = scenarios
        .iter()
        .filter(|item| item["status"].as_str() == Some("critical"))
        .count() as u64;
    let unknown = scenarios
        .iter()
        .filter(|item| item["status"].as_str() == Some("unknown"))
        .count() as u64;

    let payload = json!({
        "_observability": {
            "source_event_id": verification_run_id,
            "source_kind": "degradation_verification_run",
            "scope_project_code": "system",
            "scope_namespace_code": "degradation",
            "captured_at_epoch_ms": captured_at_epoch_ms
        },
        "degradation_verification": {
            "verification_run_id": verification_run_id,
            "scenario": scenario,
            "captured_at_epoch_ms": captured_at_epoch_ms,
            "summary": {
                "pass": pass,
                "critical": critical,
                "unknown": unknown
            },
            "scenarios": scenarios
        },
        "retrieval_science": retrieval_science::suite_metadata("degradation_verification")?,
        "degradation_policy": retrieval_science::degradation_policy_json()?,
    });
    let _ =
        postgres::insert_observability_snapshot(db, "degradation_verification", &payload).await?;
    println!("{}", serde_json::to_string_pretty(&payload)?);
    Ok(())
}

pub async fn run_load(cfg: &AppConfig, args: &VerifyLoadArgs) -> Result<()> {
    let context = normalize_engineering_context(
        &args.context,
        if args.record_live_context {
            "verify_context_pack"
        } else {
            "benchmark_context_pack"
        },
    );
    if args.workers == 0 || args.iterations_per_worker == 0 {
        return Err(anyhow!(
            "load verification requires workers > 0 and iterations_per_worker > 0"
        ));
    }

    let mut warmup_db = postgres::connect_admin(cfg).await?;
    for _ in 0..args.warmup_per_worker {
        retrieval::execute_context_pack_with_options(
            cfg,
            &mut warmup_db,
            &context,
            args.persist,
            false,
        )
        .await?;
    }

    let fast_probe = retrieval::prepare_fast_context_pack_probe(cfg, &context, args.persist)?;
    let hot_cache_only = fast_probe.is_some();
    let record_live_context = args.record_live_context;
    let publish_benchmark_snapshot = !record_live_context;

    let started = Instant::now();
    let mut handles = Vec::with_capacity(args.workers);
    for worker_index in 0..args.workers {
        let cfg = cfg.clone();
        let context = context.clone();
        let iterations = args.iterations_per_worker;
        let persist = args.persist;
        let fast_probe = fast_probe.clone();
        handles.push(tokio::spawn(async move {
            let mut samples_ns = Vec::with_capacity(iterations);
            let mut error_count = 0_usize;
            let mut cache_probe_miss_count = 0_usize;
            let mut last_stats = None;
            let needs_db = record_live_context || !hot_cache_only;
            let mut db = if needs_db {
                Some(postgres::connect_admin(&cfg).await?)
            } else {
                None
            };
            for _ in 0..iterations {
                let op_started = Instant::now();
                if hot_cache_only && !record_live_context {
                    let probe = fast_probe
                        .as_ref()
                        .expect("hot cache probe must exist for hot_cache_only mode");
                    match retrieval::fast_context_pack_probe_hit(probe) {
                        Ok(true) => {
                            samples_ns.push(op_started.elapsed().as_nanos());
                            if last_stats.is_none() {
                                last_stats = Some(probe.stats.clone());
                            }
                        }
                        Ok(false) => {
                            cache_probe_miss_count += 1;
                            error_count += 1;
                        }
                        Err(_) => {
                            error_count += 1;
                        }
                    }
                } else if record_live_context {
                    let db = db
                        .as_mut()
                        .expect("load verification db must exist when live tracking is enabled");
                    match retrieval::execute_context_pack_capture_with_options(
                        &cfg, db, &context, persist, false,
                    )
                    .await
                    {
                        Ok(result) => {
                            token_budget::record_verify_context_pack_event(db, &result.payload)
                                .await?;
                            samples_ns.push(op_started.elapsed().as_nanos());
                            last_stats = Some(result.stats);
                        }
                        Err(_) => {
                            error_count += 1;
                        }
                    }
                } else {
                    let db = db
                        .as_mut()
                        .expect("load verification db must exist when live tracking is enabled");
                    match retrieval::execute_context_pack_with_options(
                        &cfg, db, &context, persist, false,
                    )
                    .await
                    {
                        Ok(stats) => {
                            samples_ns.push(op_started.elapsed().as_nanos());
                            last_stats = Some(stats);
                        }
                        Err(_) => {
                            error_count += 1;
                        }
                    }
                }
            }
            Result::<_, anyhow::Error>::Ok(LoadWorkerOutcome {
                worker_index,
                samples_ns,
                error_count,
                cache_probe_miss_count,
                last_stats,
            })
        }));
    }

    let mut all_samples = Vec::with_capacity(args.workers * args.iterations_per_worker);
    let mut all_samples_with_workers =
        Vec::with_capacity(args.workers * args.iterations_per_worker);
    let mut total_errors = 0_usize;
    let mut total_cache_probe_misses = 0_usize;
    let mut last_stats = None;
    let mut worker_summaries = Vec::with_capacity(args.workers);
    let mut worker_p95_ms = Vec::with_capacity(args.workers);
    for handle in handles {
        let outcome = handle.await??;
        total_errors += outcome.error_count;
        total_cache_probe_misses += outcome.cache_probe_miss_count;
        for sample in &outcome.samples_ns {
            all_samples.push(*sample);
            all_samples_with_workers.push((*sample, outcome.worker_index));
        }
        if let Some(stats) = outcome.last_stats {
            last_stats = Some(stats);
        }
        let mut worker_sorted = outcome.samples_ns.clone();
        worker_sorted.sort_unstable();
        let success_count = worker_sorted.len();
        let worker_mean_ms = if success_count == 0 {
            0.0
        } else {
            sample_ns_to_ms(worker_sorted.iter().sum::<u128>()) / success_count as f64
        };
        let worker_p50_ms = if success_count == 0 {
            0.0
        } else {
            sample_ns_to_ms(percentile_sample(&worker_sorted, 50))
        };
        let worker_p95_ms_value = if success_count == 0 {
            0.0
        } else {
            sample_ns_to_ms(percentile_sample(&worker_sorted, 95))
        };
        let worker_p99_ms = if success_count == 0 {
            0.0
        } else {
            sample_ns_to_ms(percentile_sample(&worker_sorted, 99))
        };
        let worker_max_ms = worker_sorted
            .last()
            .copied()
            .map(sample_ns_to_ms)
            .unwrap_or(0.0);
        worker_p95_ms.push((outcome.worker_index, worker_p95_ms_value));
        worker_summaries.push(json!({
            "worker_index": outcome.worker_index,
            "success_count": success_count,
            "error_count": outcome.error_count,
            "cache_probe_miss_count": outcome.cache_probe_miss_count,
            "mean_ms": worker_mean_ms,
            "p50_ms": worker_p50_ms,
            "p95_ms": worker_p95_ms_value,
            "p99_ms": worker_p99_ms,
            "max_ms": worker_max_ms,
            "slowest_samples_ms": worker_sorted
                .iter()
                .rev()
                .take(5)
                .map(|sample| sample_ns_to_ms(*sample))
                .collect::<Vec<_>>(),
        }));
    }
    let wall_clock_ns = started.elapsed().as_nanos();
    let success_count = all_samples.len();
    let total_attempts = args.workers * args.iterations_per_worker;
    let error_rate = total_errors as f64 / total_attempts as f64;

    if all_samples.is_empty() {
        return Err(anyhow!("load verification produced no successful samples"));
    }
    let mut sorted = all_samples.clone();
    sorted.sort_unstable();
    let total_elapsed_ns = all_samples.iter().sum::<u128>();
    let mean_ms = sample_ns_to_ms(total_elapsed_ns) / all_samples.len() as f64;
    let p50_ms = sample_ns_to_ms(percentile_sample(&sorted, 50));
    let p95_ms = sample_ns_to_ms(percentile_sample(&sorted, 95));
    let p99_ms = sample_ns_to_ms(percentile_sample(&sorted, 99));
    let max_ms = sample_ns_to_ms(
        *sorted
            .last()
            .ok_or_else(|| anyhow!("load sample set is unexpectedly empty"))?,
    );
    let slowest_samples = {
        all_samples_with_workers.sort_unstable_by(|left, right| right.0.cmp(&left.0));
        all_samples_with_workers
            .iter()
            .take(16)
            .map(|(sample_ns, worker_index)| {
                json!({
                    "worker_index": worker_index,
                    "sample_ms": sample_ns_to_ms(*sample_ns)
                })
            })
            .collect::<Vec<_>>()
    };
    let worker_skew = if worker_p95_ms.is_empty() {
        json!({
            "fastest_worker_p95_ms": 0.0,
            "slowest_worker_p95_ms": 0.0,
            "p95_skew_ratio": 0.0
        })
    } else {
        let fastest = worker_p95_ms
            .iter()
            .min_by(|left, right| left.1.total_cmp(&right.1))
            .copied()
            .expect("worker_p95_ms is not empty");
        let slowest = worker_p95_ms
            .iter()
            .max_by(|left, right| left.1.total_cmp(&right.1))
            .copied()
            .expect("worker_p95_ms is not empty");
        json!({
            "fastest_worker_index": fastest.0,
            "fastest_worker_p95_ms": fastest.1,
            "slowest_worker_index": slowest.0,
            "slowest_worker_p95_ms": slowest.1,
            "p95_skew_ratio": if fastest.1 == 0.0 { 0.0 } else { slowest.1 / fastest.1 }
        })
    };
    let qps = if wall_clock_ns == 0 {
        success_count as f64 * 1_000_000_000.0
    } else {
        success_count as f64 * 1_000_000_000.0 / wall_clock_ns as f64
    };

    enforce_load_thresholds(args, p95_ms, qps, error_rate)?;
    let suite_key = if context.disable_cache {
        "retrieval_load_cold"
    } else {
        "retrieval_load_hot"
    };

    let last_stats =
        last_stats.ok_or_else(|| anyhow!("load verification produced no final stats"))?;
    let load_run_id = Uuid::new_v4();
    let captured_at_epoch_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock before unix epoch")?
        .as_millis() as u64;
    let payload = json!({
        "_observability": {
            "source_event_id": load_run_id,
            "source_kind": "load_verification_run",
            "scope_project_code": context.project,
            "scope_namespace_code": context.namespace,
            "captured_at_epoch_ms": captured_at_epoch_ms
        },
        "load_verification": {
            "load_run_id": load_run_id,
            "captured_at_epoch_ms": captured_at_epoch_ms,
            "project": context.project,
            "namespace": context.namespace,
            "query": context.query,
            "retrieval_mode": context.retrieval_mode,
            "disable_cache": context.disable_cache,
            "persist": args.persist,
            "workers": args.workers,
            "iterations_per_worker": args.iterations_per_worker,
            "warmup_per_worker": args.warmup_per_worker,
            "execution_mode": if hot_cache_only { "hot_cache_only" } else { "db_backed" },
            "record_live_context": record_live_context,
            "publish_benchmark_snapshot": publish_benchmark_snapshot,
            "success_count": success_count,
            "error_count": total_errors,
            "error_rate": error_rate,
            "wall_clock_ms": sample_ns_to_ms(wall_clock_ns),
            "wall_clock_ns": wall_clock_ns,
            "sample_resolution": "ns",
            "samples_ns": all_samples,
            "samples_us": all_samples
                .iter()
                .map(|sample_ns| sample_ns / 1000)
                .collect::<Vec<_>>(),
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
        },
        "load_diagnostics": {
            "worker_skew": worker_skew,
            "per_worker": worker_summaries,
            "slowest_samples": slowest_samples,
            "path_uniformity": {
                "hot_cache_only": hot_cache_only,
                "record_live_context": record_live_context,
                "cache_probe_miss_count": total_cache_probe_misses,
                "db_fallback_count": 0
            }
        },
        "retrieval_science": retrieval_science::suite_metadata(suite_key)?,
        "degradation_policy": retrieval_science::degradation_policy_json()?
    });
    if publish_benchmark_snapshot {
        let snapshot_kind = if context.disable_cache {
            "retrieval_load_cold"
        } else {
            "retrieval_load_hot"
        };
        let db = postgres::connect_admin(cfg).await?;
        let _ = postgres::insert_observability_snapshot(&db, snapshot_kind, &payload).await?;
    }
    println!("{}", serde_json::to_string_pretty(&payload)?);
    Ok(())
}

pub async fn run_token_benchmark(
    cfg: &AppConfig,
    db: &mut Client,
    args: &VerifyTokenBenchmarkArgs,
) -> Result<()> {
    let mut normalized = args.clone();
    normalized.context = normalize_engineering_context(&args.context, "verify_context_pack");
    let payload = collect_token_benchmark(cfg, db, &normalized).await?;
    println!("{}", serde_json::to_string_pretty(&payload)?);
    Ok(())
}

pub async fn run_token_benchmark_suite(
    cfg: &AppConfig,
    db: &mut Client,
    args: &VerifyTokenBenchmarkSuiteArgs,
) -> Result<()> {
    let queries = collect_suite_queries(&args.query, args.queries_file.as_deref())?;
    if queries.is_empty() {
        return Err(anyhow!(
            "token benchmark suite requires at least one query via --query or --queries-file"
        ));
    }

    let mut runs = Vec::with_capacity(queries.len());
    let mut factor_samples = Vec::with_capacity(queries.len());
    let mut percent_samples = Vec::with_capacity(queries.len());
    let mut saved_token_samples = Vec::with_capacity(queries.len());
    let mut naive_token_samples = Vec::with_capacity(queries.len());
    let mut context_token_samples = Vec::with_capacity(queries.len());

    for query in queries {
        let payload = collect_token_benchmark(
            cfg,
            db,
            &VerifyTokenBenchmarkArgs {
                context: ContextPackArgs {
                    project: args.project.clone(),
                    namespace: args.namespace.clone(),
                    query: query.clone(),
                    retrieval_mode: args.retrieval_mode.clone(),
                    disable_cache: args.disable_cache,
                    limit_documents: args.limit_documents,
                    limit_symbols: args.limit_symbols,
                    limit_chunks: args.limit_chunks,
                    limit_semantic_chunks: args.limit_semantic_chunks,
                    token_source_kind: "verify_context_pack".to_string(),
                    client_prompt_tokens: None,
                    assistant_generation_tokens: None,
                    tool_overhead_tokens: None,
                    continuity_restore_tokens: None,
                },
                tokenizer: args.tokenizer.clone(),
                naive_limit_files: args.naive_limit_files,
                naive_max_bytes_per_file: args.naive_max_bytes_per_file,
                min_savings_factor: 0.0,
                min_savings_percent: 0.0,
            },
        )
        .await?;

        let benchmark = &payload["token_benchmark"];
        let savings = &benchmark["savings"];
        let naive_scope = &benchmark["naive_scope"];
        let compact = &benchmark["context_pack_render"];

        let savings_factor = savings["savings_factor"]
            .as_f64()
            .ok_or_else(|| anyhow!("token benchmark payload missing savings_factor"))?;
        let savings_percent = savings["savings_percent"]
            .as_f64()
            .ok_or_else(|| anyhow!("token benchmark payload missing savings_percent"))?;
        let saved_tokens = savings["saved_tokens"]
            .as_u64()
            .ok_or_else(|| anyhow!("token benchmark payload missing saved_tokens"))?;
        let naive_tokens = naive_scope["tokens"]
            .as_u64()
            .ok_or_else(|| anyhow!("token benchmark payload missing naive tokens"))?;
        let context_tokens = compact["tokens"]
            .as_u64()
            .ok_or_else(|| anyhow!("token benchmark payload missing compact tokens"))?;

        factor_samples.push(savings_factor);
        percent_samples.push(savings_percent);
        saved_token_samples.push(saved_tokens as f64);
        naive_token_samples.push(naive_tokens as f64);
        context_token_samples.push(context_tokens as f64);
        runs.push(benchmark.clone());
    }

    let mean_savings_factor = mean_f64(&factor_samples);
    let mean_savings_percent = mean_f64(&percent_samples);
    let mean_saved_tokens = mean_f64(&saved_token_samples);
    let mean_naive_tokens = mean_f64(&naive_token_samples);
    let mean_context_tokens = mean_f64(&context_token_samples);
    let p50_savings_factor = percentile_f64(&factor_samples, 50);
    let p95_savings_factor = percentile_f64(&factor_samples, 95);
    let p50_savings_percent = percentile_f64(&percent_samples, 50);
    let p95_savings_percent = percentile_f64(&percent_samples, 95);

    let mut violations = Vec::new();
    if mean_savings_factor < args.min_mean_savings_factor {
        violations.push(format!(
            "mean_savings_factor={mean_savings_factor:.3} below {:.3}",
            args.min_mean_savings_factor
        ));
    }
    if mean_savings_percent < args.min_mean_savings_percent {
        violations.push(format!(
            "mean_savings_percent={mean_savings_percent:.3} below {:.3}",
            args.min_mean_savings_percent
        ));
    }
    if !violations.is_empty() {
        return Err(anyhow!(
            "token benchmark suite thresholds violated: {}",
            violations.join("; ")
        ));
    }

    let payload = json!({
        "token_benchmark_suite": {
            "project": args.project,
            "namespace": args.namespace,
            "retrieval_mode": args.retrieval_mode,
            "tokenizer": args.tokenizer,
            "disable_cache": args.disable_cache,
            "queries_total": runs.len(),
            "mean_savings_factor": mean_savings_factor,
            "p50_savings_factor": p50_savings_factor,
            "p95_savings_factor": p95_savings_factor,
            "mean_savings_percent": mean_savings_percent,
            "p50_savings_percent": p50_savings_percent,
            "p95_savings_percent": p95_savings_percent,
            "mean_saved_tokens": mean_saved_tokens,
            "mean_naive_tokens": mean_naive_tokens,
            "mean_context_tokens": mean_context_tokens,
            "runs": runs,
        },
        "retrieval_science": retrieval_science::suite_metadata("token_benchmark_suite")?,
        "degradation_policy": retrieval_science::degradation_policy_json()?,
    });
    let _ = postgres::insert_observability_snapshot(db, "token_benchmark_suite", &payload).await?;
    println!("{}", serde_json::to_string_pretty(&payload)?);
    Ok(())
}

#[derive(Debug, Clone, Deserialize)]
struct TextCompareCase {
    query: String,
    #[serde(default)]
    expected_projects: Vec<String>,
    #[serde(default)]
    expected_paths: Vec<String>,
    #[serde(default)]
    expected_terms: Vec<String>,
    #[serde(default)]
    expected_symbols: Vec<String>,
    #[serde(default)]
    description: Option<String>,
}

#[derive(Debug, Clone)]
struct StrategyOutcome {
    precision: f64,
    hit: bool,
    head_hit: bool,
    total_items: usize,
    matched_items: usize,
    prompt_tokens: usize,
}

#[derive(Debug, Clone)]
struct TextCompareEvalProbe {
    name: String,
    strategy: &'static str,
    query: String,
    verdict_class: String,
    verdict_reason: String,
    details: Value,
}

pub async fn run_text_compare(
    cfg: &AppConfig,
    db: &mut Client,
    args: &VerifyTextCompareArgs,
) -> Result<()> {
    let default_cases_path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/text_compare_cases.jsonl");
    let cases_path = args
        .cases_file
        .as_deref()
        .unwrap_or(default_cases_path.as_path());
    let cases = collect_text_compare_cases(cases_path)?;
    if cases.is_empty() {
        return Err(anyhow!(
            "text compare requires at least one case in {}",
            cases_path.display()
        ));
    }

    let tokenizer = build_tokenizer(&args.tokenizer)?;
    let mut runs = Vec::with_capacity(cases.len());
    let mut hybrid_precisions = Vec::with_capacity(cases.len());
    let mut lexical_precisions = Vec::with_capacity(cases.len());
    let mut semantic_precisions = Vec::with_capacity(cases.len());
    let mut hybrid_hits = Vec::with_capacity(cases.len());
    let mut lexical_hits = Vec::with_capacity(cases.len());
    let mut semantic_hits = Vec::with_capacity(cases.len());
    let mut hybrid_head_hits = Vec::with_capacity(cases.len());
    let mut lexical_head_hits = Vec::with_capacity(cases.len());
    let mut semantic_head_hits = Vec::with_capacity(cases.len());
    let mut hybrid_tokens = Vec::with_capacity(cases.len());
    let mut lexical_tokens = Vec::with_capacity(cases.len());
    let mut semantic_tokens = Vec::with_capacity(cases.len());
    let mut naive_tokens = Vec::with_capacity(cases.len());
    let mut hybrid_savings_factors = Vec::with_capacity(cases.len());
    let mut eval_probes = Vec::with_capacity(cases.len() * 3);

    for case in cases {
        let context = ContextPackArgs {
            project: args.project.clone(),
            namespace: args.namespace.clone(),
            query: case.query.clone(),
            retrieval_mode: args.retrieval_mode.clone(),
            disable_cache: args.disable_cache,
            limit_documents: args.limit_documents,
            limit_symbols: args.limit_symbols,
            limit_chunks: args.limit_chunks,
            limit_semantic_chunks: args.limit_semantic_chunks,
            token_source_kind: "verify_context_pack".to_string(),
            client_prompt_tokens: None,
            assistant_generation_tokens: None,
            tool_overhead_tokens: None,
            continuity_restore_tokens: None,
        };
        let pack =
            retrieval::execute_context_pack_capture_with_options(cfg, db, &context, false, false)
                .await?;
        let naive_scope = collect_naive_scope(
            &pack.payload,
            args.naive_limit_files,
            args.naive_max_bytes_per_file,
        )?;

        let hybrid_prompt = render_context_pack_prompt(&pack.payload);
        let lexical_prompt = render_filtered_context_prompt(&pack.payload, true, true, true, false);
        let semantic_prompt =
            render_filtered_context_prompt(&pack.payload, false, false, false, true);
        let naive_prompt = render_naive_scope_prompt(&pack.payload, &naive_scope);

        let hybrid = evaluate_strategy(
            collect_strategy_items(&pack.payload, StrategySelection::Hybrid),
            &case,
            tokenizer.encode_with_special_tokens(&hybrid_prompt).len(),
        );
        let lexical = evaluate_strategy(
            collect_strategy_items(&pack.payload, StrategySelection::LexicalOnly),
            &case,
            tokenizer.encode_with_special_tokens(&lexical_prompt).len(),
        );
        let semantic = evaluate_strategy(
            collect_strategy_items(&pack.payload, StrategySelection::SemanticOnly),
            &case,
            tokenizer.encode_with_special_tokens(&semantic_prompt).len(),
        );
        let naive_prompt_tokens = tokenizer.encode_with_special_tokens(&naive_prompt).len();
        let hybrid_savings_factor = if hybrid.prompt_tokens == 0 {
            naive_prompt_tokens as f64
        } else {
            naive_prompt_tokens as f64 / hybrid.prompt_tokens as f64
        };
        let hybrid_eval = text_compare_eval_probe("hybrid", &case, &hybrid)?;
        let lexical_eval = text_compare_eval_probe("lexical_only", &case, &lexical)?;
        let semantic_eval = text_compare_eval_probe("semantic_only", &case, &semantic)?;

        hybrid_precisions.push(hybrid.precision);
        lexical_precisions.push(lexical.precision);
        semantic_precisions.push(semantic.precision);
        hybrid_hits.push(hybrid.hit as usize as f64);
        lexical_hits.push(lexical.hit as usize as f64);
        semantic_hits.push(semantic.hit as usize as f64);
        hybrid_head_hits.push(hybrid.head_hit as usize as f64);
        lexical_head_hits.push(lexical.head_hit as usize as f64);
        semantic_head_hits.push(semantic.head_hit as usize as f64);
        hybrid_tokens.push(hybrid.prompt_tokens as f64);
        lexical_tokens.push(lexical.prompt_tokens as f64);
        semantic_tokens.push(semantic.prompt_tokens as f64);
        naive_tokens.push(naive_prompt_tokens as f64);
        hybrid_savings_factors.push(hybrid_savings_factor);
        eval_probes.push(hybrid_eval.clone());
        eval_probes.push(lexical_eval.clone());
        eval_probes.push(semantic_eval.clone());

        runs.push(json!({
            "query": case.query,
            "description": case.description,
            "expected": {
                "projects": case.expected_projects,
                "paths": case.expected_paths,
                "terms": case.expected_terms,
                "symbols": case.expected_symbols,
            },
            "strategies": {
                "hybrid": strategy_to_json(&hybrid, &hybrid_eval),
                "lexical_only": strategy_to_json(&lexical, &lexical_eval),
                "semantic_only": strategy_to_json(&semantic, &semantic_eval),
            },
            "token_budget": {
                "hybrid_prompt_tokens": hybrid.prompt_tokens,
                "lexical_prompt_tokens": lexical.prompt_tokens,
                "semantic_prompt_tokens": semantic.prompt_tokens,
                "naive_prompt_tokens": naive_prompt_tokens,
                "hybrid_savings_factor_vs_naive": hybrid_savings_factor,
            },
            "visible_projects": pack.payload["visible_projects"].clone(),
        }));
    }

    let mean_hybrid_precision = mean_f64(&hybrid_precisions);
    let mean_lexical_precision = mean_f64(&lexical_precisions);
    let mean_semantic_precision = mean_f64(&semantic_precisions);
    let hybrid_hit_ratio = mean_f64(&hybrid_hits);
    let lexical_hit_ratio = mean_f64(&lexical_hits);
    let semantic_hit_ratio = mean_f64(&semantic_hits);
    let hybrid_head_hit_ratio = mean_f64(&hybrid_head_hits);
    let lexical_head_hit_ratio = mean_f64(&lexical_head_hits);
    let semantic_head_hit_ratio = mean_f64(&semantic_head_hits);
    let mean_hybrid_tokens = mean_f64(&hybrid_tokens);
    let mean_lexical_tokens = mean_f64(&lexical_tokens);
    let mean_semantic_tokens = mean_f64(&semantic_tokens);
    let mean_naive_tokens = mean_f64(&naive_tokens);
    let mean_hybrid_savings_factor = mean_f64(&hybrid_savings_factors);
    let canonical_eval = build_text_compare_canonical_eval(&eval_probes)?;

    let mut violations = Vec::new();
    if hybrid_hit_ratio < args.min_hybrid_hit_ratio {
        violations.push(format!(
            "hybrid_hit_ratio={hybrid_hit_ratio:.3} below {:.3}",
            args.min_hybrid_hit_ratio
        ));
    }
    if hybrid_head_hit_ratio < args.min_hybrid_head_hit_ratio {
        violations.push(format!(
            "hybrid_head_hit_ratio={hybrid_head_hit_ratio:.3} below {:.3}",
            args.min_hybrid_head_hit_ratio
        ));
    }
    if mean_hybrid_savings_factor < args.min_hybrid_savings_factor {
        violations.push(format!(
            "mean_hybrid_savings_factor={mean_hybrid_savings_factor:.3} below {:.3}",
            args.min_hybrid_savings_factor
        ));
    }
    if !violations.is_empty() {
        return Err(anyhow!(
            "text compare thresholds violated: {}",
            violations.join("; ")
        ));
    }

    let payload = json!({
        "text_compare": {
            "project": args.project,
            "namespace": args.namespace,
            "retrieval_mode": args.retrieval_mode,
            "cases_file": cases_path.display().to_string(),
            "cases_total": runs.len(),
            "tokenizer": args.tokenizer,
            "mean_precision": {
                "hybrid": mean_hybrid_precision,
                "lexical_only": mean_lexical_precision,
                "semantic_only": mean_semantic_precision,
            },
            "case_hit_ratio": {
                "hybrid": hybrid_hit_ratio,
                "lexical_only": lexical_hit_ratio,
                "semantic_only": semantic_hit_ratio,
            },
            "head_hit_ratio": {
                "hybrid": hybrid_head_hit_ratio,
                "lexical_only": lexical_head_hit_ratio,
                "semantic_only": semantic_head_hit_ratio,
            },
            "mean_prompt_tokens": {
                "hybrid": mean_hybrid_tokens,
                "lexical_only": mean_lexical_tokens,
                "semantic_only": mean_semantic_tokens,
                "naive": mean_naive_tokens,
            },
            "mean_hybrid_savings_factor_vs_naive": mean_hybrid_savings_factor,
            "canonical_eval": canonical_eval,
            "runs": runs,
        },
        "retrieval_science": retrieval_science::suite_metadata("text_compare")?,
        "degradation_policy": retrieval_science::degradation_policy_json()?,
    });
    let _ = postgres::insert_observability_snapshot(db, "text_compare", &payload).await?;
    println!("{}", serde_json::to_string_pretty(&payload)?);
    Ok(())
}

pub async fn collect_token_benchmark(
    cfg: &AppConfig,
    db: &mut Client,
    args: &VerifyTokenBenchmarkArgs,
) -> Result<Value> {
    let pack =
        retrieval::execute_context_pack_capture_with_options(cfg, db, &args.context, false, false)
            .await?;
    let tokenizer = build_tokenizer(&args.tokenizer)?;
    let naive_scope = collect_naive_scope(
        &pack.payload,
        args.naive_limit_files,
        args.naive_max_bytes_per_file,
    )?;
    let naive_prompt = render_naive_scope_prompt(&pack.payload, &naive_scope);
    let context_prompt = render_context_pack_prompt(&pack.payload);

    let naive_tokens = tokenizer.encode_with_special_tokens(&naive_prompt).len();
    let context_tokens = tokenizer.encode_with_special_tokens(&context_prompt).len();
    let saved_tokens = naive_tokens.saturating_sub(context_tokens);
    let savings_factor = if context_tokens == 0 {
        naive_tokens as f64
    } else {
        naive_tokens as f64 / context_tokens as f64
    };
    let savings_percent = if naive_tokens == 0 {
        0.0
    } else {
        saved_tokens as f64 * 100.0 / naive_tokens as f64
    };
    enforce_token_benchmark_thresholds(args, savings_factor, savings_percent)?;

    let payload = json!({
        "token_benchmark": {
            "project": args.context.project,
            "namespace": args.context.namespace,
            "query": args.context.query,
            "retrieval_mode": args.context.retrieval_mode,
            "tokenizer": args.tokenizer,
            "naive_limit_files": args.naive_limit_files,
            "naive_max_bytes_per_file": args.naive_max_bytes_per_file,
            "visible_projects": pack.payload["visible_projects"].clone(),
            "naive_scope": {
                "files_considered": naive_scope.files.len(),
                "files": naive_scope.files,
                "rendered_bytes": naive_prompt.len(),
                "tokens": naive_tokens,
            },
            "context_pack_render": {
                "rendered_bytes": context_prompt.len(),
                "tokens": context_tokens,
            },
            "savings": {
                "saved_tokens": saved_tokens,
                "savings_factor": savings_factor,
                "savings_percent": savings_percent,
            }
        }
    });
    let _ = postgres::insert_observability_snapshot(db, "token_benchmark", &payload).await?;
    token_budget::record_verify_benchmark_event(db, &payload).await?;
    Ok(payload)
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
            },
            "degradation_policy": retrieval_science::degradation_policy_json()?,
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

    let report = compatibility::check_fresh(cfg).await?;
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

    let failed_closed = match compatibility::check_fresh(cfg).await {
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

fn sample_ns_to_ms(sample_ns: u128) -> f64 {
    sample_ns as f64 / 1_000_000.0
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

fn enforce_token_benchmark_thresholds(
    args: &VerifyTokenBenchmarkArgs,
    savings_factor: f64,
    savings_percent: f64,
) -> Result<()> {
    let mut violations = Vec::new();
    if savings_factor < args.min_savings_factor {
        violations.push(format!(
            "savings_factor={savings_factor:.3} below {:.3}",
            args.min_savings_factor
        ));
    }
    if savings_percent < args.min_savings_percent {
        violations.push(format!(
            "savings_percent={savings_percent:.3} below {:.3}",
            args.min_savings_percent
        ));
    }

    if violations.is_empty() {
        return Ok(());
    }

    Err(anyhow!(
        "token benchmark thresholds violated: {}",
        violations.join("; ")
    ))
}

fn collect_suite_queries(
    inline_queries: &[String],
    queries_file: Option<&Path>,
) -> Result<Vec<String>> {
    let mut seen = HashSet::new();
    let mut queries = Vec::new();

    for query in inline_queries {
        let normalized = query.trim();
        if normalized.is_empty() || !seen.insert(normalized.to_string()) {
            continue;
        }
        queries.push(normalized.to_string());
    }

    if let Some(path) = queries_file {
        let content = fs::read_to_string(path)
            .with_context(|| format!("failed to read queries file {}", path.display()))?;
        for line in content.lines() {
            let normalized = line.trim();
            if normalized.is_empty() || normalized.starts_with('#') {
                continue;
            }
            if !seen.insert(normalized.to_string()) {
                continue;
            }
            queries.push(normalized.to_string());
        }
    }

    Ok(queries)
}

fn mean_f64(samples: &[f64]) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    samples.iter().sum::<f64>() / samples.len() as f64
}

fn percentile_f64(samples: &[f64], percentile: usize) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    let mut sorted = samples.to_vec();
    sorted.sort_by(f64::total_cmp);
    let percentile = percentile.min(100);
    let rank = (percentile * sorted.len()).div_ceil(100);
    let index = rank.saturating_sub(1).min(sorted.len() - 1);
    sorted[index]
}

#[derive(Debug)]
struct NaiveScopeFile {
    project_code: String,
    relative_path: String,
    original_bytes: usize,
    bytes_used: usize,
    truncated: bool,
    content: String,
}

#[derive(Debug)]
struct NaiveScope {
    files: Vec<Value>,
    rendered_files: Vec<NaiveScopeFile>,
}

fn collect_naive_scope(
    payload: &Value,
    limit_files: usize,
    max_bytes_per_file: usize,
) -> Result<NaiveScope> {
    let mut files = Vec::new();
    for project in payload["visible_projects"].as_array().into_iter().flatten() {
        let Some(project_code) = project["project_code"].as_str() else {
            continue;
        };
        let Some(repo_root) = project["repo_root"].as_str() else {
            continue;
        };
        for path in collect_scope_files(Path::new(repo_root), limit_files)? {
            let relative_path = path
                .strip_prefix(repo_root)
                .unwrap_or(path.as_path())
                .display()
                .to_string();
            let bytes = fs::read(&path)
                .with_context(|| format!("failed to read naive scope file {}", path.display()))?;
            let original_bytes = bytes.len();
            let bytes_used = original_bytes.min(max_bytes_per_file);
            let content = safe_lossy_prefix(&bytes, bytes_used);
            files.push(NaiveScopeFile {
                project_code: project_code.to_string(),
                relative_path,
                original_bytes,
                bytes_used: content.len(),
                truncated: original_bytes > content.len(),
                content,
            });
        }
    }

    files.sort_by(|left, right| {
        left.project_code
            .cmp(&right.project_code)
            .then_with(|| left.relative_path.cmp(&right.relative_path))
    });
    if limit_files > 0 {
        files.truncate(limit_files);
    }

    let metadata = files
        .iter()
        .map(|file| {
            json!({
                "project_code": file.project_code,
                "relative_path": file.relative_path,
                "original_bytes": file.original_bytes,
                "bytes_used": file.bytes_used,
                "truncated": file.truncated,
            })
        })
        .collect();

    Ok(NaiveScope {
        files: metadata,
        rendered_files: files,
    })
}

fn collect_scope_files(root: &Path, limit_files: usize) -> Result<Vec<std::path::PathBuf>> {
    let mut builder = WalkBuilder::new(root);
    builder
        .standard_filters(true)
        .hidden(false)
        .git_ignore(true)
        .git_exclude(true)
        .git_global(true);
    let mut files = builder
        .build()
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_type()
                .map(|kind| kind.is_file())
                .unwrap_or(false)
        })
        .map(|entry| entry.into_path())
        .filter(|path| language::detect(path).is_some())
        .collect::<Vec<_>>();
    files.sort();
    if limit_files > 0 {
        files.truncate(limit_files);
    }
    Ok(files)
}

fn safe_lossy_prefix(bytes: &[u8], max_bytes: usize) -> String {
    let slice = &bytes[..bytes.len().min(max_bytes)];
    String::from_utf8_lossy(slice).into_owned()
}

fn render_naive_scope_prompt(payload: &Value, scope: &NaiveScope) -> String {
    let mut prompt = String::new();
    prompt.push_str("NAIVE_SCOPE\n");
    prompt.push_str(
        "This bundle represents the visible project scope without retrieval reduction.\n",
    );
    prompt.push_str("Query: ");
    prompt.push_str(payload["query"].as_str().unwrap_or_default());
    prompt.push_str("\nVisible projects:\n");
    for project in payload["visible_projects"].as_array().into_iter().flatten() {
        prompt.push_str("- ");
        prompt.push_str(project["project_code"].as_str().unwrap_or_default());
        prompt.push_str(" :: ");
        prompt.push_str(project["repo_root"].as_str().unwrap_or_default());
        prompt.push('\n');
    }
    prompt.push('\n');
    for file in &scope.rendered_files {
        prompt.push_str("## PROJECT ");
        prompt.push_str(&file.project_code);
        prompt.push('\n');
        prompt.push_str("### FILE ");
        prompt.push_str(&file.relative_path);
        prompt.push('\n');
        prompt.push_str(&file.content);
        prompt.push_str("\n\n");
    }
    prompt
}

fn render_context_pack_prompt(payload: &Value) -> String {
    let mut excerpt_paths = HashSet::new();
    let mut exact_lines = Vec::new();
    let mut symbol_lines = Vec::new();
    let mut seen_symbols = HashSet::new();
    for item in payload["retrieval"]["symbol_hits"]
        .as_array()
        .into_iter()
        .flatten()
    {
        let line = format!(
            "[{}] {} :: {} :: {}",
            item["provenance"]["source_project"]
                .as_str()
                .unwrap_or_default(),
            item["relative_path"].as_str().unwrap_or_default(),
            item["name"].as_str().unwrap_or_default(),
            item["kind"].as_str().unwrap_or_default(),
        );
        if seen_symbols.insert(line.clone()) {
            symbol_lines.push(line);
        }
    }

    let mut excerpt_lines = Vec::new();
    let mut seen_excerpts = HashSet::new();
    for section in ["lexical_chunks", "semantic_chunks"] {
        for item in payload["retrieval"][section]
            .as_array()
            .into_iter()
            .flatten()
        {
            let line = format!(
                "[{}] {} :: {}",
                item["provenance"]["source_project"]
                    .as_str()
                    .or_else(|| item["project_code"].as_str())
                    .unwrap_or_default(),
                item["relative_path"].as_str().unwrap_or_default(),
                item["content"].as_str().unwrap_or_default(),
            );
            if seen_excerpts.insert(line.clone()) {
                excerpt_lines.push(line);
            }
            excerpt_paths.insert(format!(
                "{}::{}",
                item["provenance"]["source_project"]
                    .as_str()
                    .or_else(|| item["project_code"].as_str())
                    .unwrap_or_default(),
                item["relative_path"].as_str().unwrap_or_default(),
            ));
        }
    }

    let mut seen_exact = HashSet::new();
    for item in payload["retrieval"]["exact_documents"]
        .as_array()
        .into_iter()
        .flatten()
    {
        let path_key = format!(
            "{}::{}",
            item["project_code"].as_str().unwrap_or_default(),
            item["relative_path"].as_str().unwrap_or_default(),
        );
        if excerpt_paths.contains(&path_key) {
            continue;
        }
        let line = format!(
            "[{}] {} {}",
            item["project_code"].as_str().unwrap_or_default(),
            item["relative_path"].as_str().unwrap_or_default(),
            item["snippet"].as_str().unwrap_or_default(),
        );
        if seen_exact.insert(line.clone()) {
            exact_lines.push(line);
        }
    }

    let mut prompt = String::new();
    prompt.push('Q');
    prompt.push(':');
    prompt.push_str(payload["query"].as_str().unwrap_or_default());
    prompt.push('\n');
    prompt.push('P');
    prompt.push(':');
    prompt.push_str(payload["project"]["code"].as_str().unwrap_or_default());
    prompt.push('\n');
    prompt.push('M');
    prompt.push(':');
    prompt.push_str(
        payload["effective_retrieval_mode"]
            .as_str()
            .unwrap_or_default(),
    );
    prompt.push_str("\n\n");
    push_compact_lines(&mut prompt, "D", &exact_lines);
    push_compact_lines(&mut prompt, "S", &symbol_lines);
    push_compact_lines(&mut prompt, "E", &excerpt_lines);
    prompt
}

#[derive(Debug, Clone, Copy)]
enum StrategySelection {
    Hybrid,
    LexicalOnly,
    SemanticOnly,
}

fn collect_text_compare_cases(path: &Path) -> Result<Vec<TextCompareCase>> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read text compare cases {}", path.display()))?;
    let mut cases = Vec::new();
    for line in content.lines() {
        let normalized = line.trim();
        if normalized.is_empty() || normalized.starts_with('#') {
            continue;
        }
        let case: TextCompareCase =
            serde_json::from_str(normalized).context("failed to parse text compare case")?;
        if case.query.trim().is_empty() {
            return Err(anyhow!("text compare case query must not be empty"));
        }
        if case.expected_projects.is_empty()
            && case.expected_paths.is_empty()
            && case.expected_terms.is_empty()
            && case.expected_symbols.is_empty()
        {
            return Err(anyhow!(
                "text compare case must declare at least one expected signal"
            ));
        }
        cases.push(case);
    }
    Ok(cases)
}

fn collect_strategy_items(payload: &Value, strategy: StrategySelection) -> Vec<Value> {
    let retrieval = &payload["retrieval"];
    match strategy {
        StrategySelection::Hybrid => [
            &retrieval["exact_documents"],
            &retrieval["symbol_hits"],
            &retrieval["lexical_chunks"],
            &retrieval["semantic_chunks"],
        ]
        .into_iter()
        .flat_map(|items| items.as_array().into_iter().flatten().cloned())
        .collect(),
        StrategySelection::LexicalOnly => [
            &retrieval["exact_documents"],
            &retrieval["symbol_hits"],
            &retrieval["lexical_chunks"],
        ]
        .into_iter()
        .flat_map(|items| items.as_array().into_iter().flatten().cloned())
        .collect(),
        StrategySelection::SemanticOnly => retrieval["semantic_chunks"]
            .as_array()
            .into_iter()
            .flatten()
            .cloned()
            .collect(),
    }
}

fn evaluate_strategy(
    items: Vec<Value>,
    case: &TextCompareCase,
    prompt_tokens: usize,
) -> StrategyOutcome {
    let matched_items = items
        .iter()
        .filter(|item| item_matches_text_compare_case(item, case))
        .count();
    let total_items = items.len();
    let precision = if total_items == 0 {
        0.0
    } else {
        matched_items as f64 / total_items as f64
    };
    StrategyOutcome {
        precision,
        hit: matched_items > 0,
        head_hit: items
            .iter()
            .take(3)
            .any(|item| item_matches_text_compare_case(item, case)),
        total_items,
        matched_items,
        prompt_tokens,
    }
}

fn strategy_to_json(outcome: &StrategyOutcome, eval: &TextCompareEvalProbe) -> Value {
    json!({
        "precision": outcome.precision,
        "hit": outcome.hit,
        "head_hit": outcome.head_hit,
        "total_items": outcome.total_items,
        "matched_items": outcome.matched_items,
        "prompt_tokens": outcome.prompt_tokens,
        "eval_verdict_class": eval.verdict_class,
        "eval_reason": eval.verdict_reason,
    })
}

fn text_compare_eval_probe(
    strategy: &'static str,
    case: &TextCompareCase,
    outcome: &StrategyOutcome,
) -> Result<TextCompareEvalProbe> {
    let verdict = eval_verdict::derive_eval_verdict(
        EvalPattern::RetrievalTarget,
        &EvalSignals {
            expected_present: Some(outcome.hit),
            unexpected_present: outcome.total_items > outcome.matched_items,
            has_expected_target: true,
            ..EvalSignals::default()
        },
    )?;
    Ok(TextCompareEvalProbe {
        name: format!("{strategy}:{}", case.query),
        strategy,
        query: case.query.clone(),
        verdict_class: verdict.class_key,
        verdict_reason: verdict.reason,
        details: json!({
            "description": case.description,
            "expected": {
                "projects": case.expected_projects,
                "paths": case.expected_paths,
                "terms": case.expected_terms,
                "symbols": case.expected_symbols,
            },
            "hit": outcome.hit,
            "head_hit": outcome.head_hit,
            "matched_items": outcome.matched_items,
            "total_items": outcome.total_items,
            "precision": outcome.precision,
            "prompt_tokens": outcome.prompt_tokens,
            "unexpected_present": outcome.total_items > outcome.matched_items,
        }),
    })
}

fn build_text_compare_canonical_eval(probes: &[TextCompareEvalProbe]) -> Result<Value> {
    let mut summary = eval_verdict::summarize_eval_layer(
        probes.iter().map(|probe| probe.verdict_class.as_str()),
    )?;
    let mut strategy_groups = BTreeMap::<&'static str, Vec<&TextCompareEvalProbe>>::new();
    for probe in probes {
        strategy_groups
            .entry(probe.strategy)
            .or_default()
            .push(probe);
    }
    let mut strategy_breakdown = serde_json::Map::new();
    for (strategy, probes) in strategy_groups {
        strategy_breakdown.insert(
            strategy.to_string(),
            eval_verdict::summarize_eval_layer(
                probes.iter().map(|probe| probe.verdict_class.as_str()),
            )?,
        );
    }
    summary["strategy_breakdown"] = Value::Object(strategy_breakdown);
    summary["probes"] = json!(
        probes
            .iter()
            .map(|probe| {
                json!({
                    "name": probe.name,
                    "strategy": probe.strategy,
                    "query": probe.query,
                    "eval_verdict_class": probe.verdict_class,
                    "eval_reason": probe.verdict_reason,
                    "details": probe.details,
                })
            })
            .collect::<Vec<_>>()
    );
    Ok(summary)
}

fn item_matches_text_compare_case(item: &Value, case: &TextCompareCase) -> bool {
    let project_ok = case.expected_projects.is_empty()
        || item_project_code(item).is_some_and(|project| {
            case.expected_projects
                .iter()
                .any(|expected| expected == project)
        });
    let path_ok = case.expected_paths.is_empty()
        || item_relative_path(item)
            .is_some_and(|path| case.expected_paths.iter().any(|expected| expected == path));
    let term_ok = case.expected_terms.is_empty()
        || case
            .expected_terms
            .iter()
            .all(|term| item_contains_text(item, term));
    let symbol_ok = case.expected_symbols.is_empty()
        || item["name"].as_str().is_some_and(|name| {
            case.expected_symbols
                .iter()
                .any(|expected| expected == name)
        });
    project_ok && path_ok && term_ok && symbol_ok
}

fn item_project_code(item: &Value) -> Option<&str> {
    item["project_code"]
        .as_str()
        .or_else(|| item["provenance"]["source_project"].as_str())
}

fn item_relative_path(item: &Value) -> Option<&str> {
    item["relative_path"]
        .as_str()
        .or_else(|| item["provenance"]["path"].as_str())
}

fn item_contains_text(item: &Value, expected: &str) -> bool {
    let expected_lower = expected.to_lowercase();
    [
        item["snippet"].as_str(),
        item["content"].as_str(),
        item["name"].as_str(),
        item["relative_path"].as_str(),
    ]
    .into_iter()
    .flatten()
    .any(|value| value.to_lowercase().contains(&expected_lower))
}

fn render_filtered_context_prompt(
    payload: &Value,
    include_exact: bool,
    include_symbols: bool,
    include_lexical: bool,
    include_semantic: bool,
) -> String {
    let mut filtered = payload.clone();
    if !include_exact {
        filtered["retrieval"]["exact_documents"] = json!([]);
    }
    if !include_symbols {
        filtered["retrieval"]["symbol_hits"] = json!([]);
    }
    if !include_lexical {
        filtered["retrieval"]["lexical_chunks"] = json!([]);
    }
    if !include_semantic {
        filtered["retrieval"]["semantic_chunks"] = json!([]);
    }
    render_context_pack_prompt(&filtered)
}

fn push_compact_lines(prompt: &mut String, title: &str, lines: &[String]) {
    prompt.push_str(title);
    prompt.push('\n');
    for line in lines {
        prompt.push_str(line);
        prompt.push('\n');
    }
    prompt.push('\n');
}

fn build_tokenizer(name: &str) -> Result<CoreBPE> {
    match name {
        "o200k_base" => o200k_base().context("failed to initialize o200k_base tokenizer"),
        "cl100k_base" => cl100k_base().context("failed to initialize cl100k_base tokenizer"),
        other => Err(anyhow!("unsupported tokenizer: {other}")),
    }
}

fn load_accuracy_suite(path: &Path) -> Result<AccuracySuiteManifest> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read accuracy suite manifest {}", path.display()))?;
    let file: AccuracySuiteFile =
        toml::from_str(&content).context("failed to parse accuracy suite manifest")?;
    Ok(file.suite)
}

fn collect_visible_projects(payload: &Value) -> Vec<String> {
    payload["visible_projects"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item["project_code"].as_str().map(ToOwned::to_owned))
        .collect()
}

fn collect_visible_namespaces(payload: &Value) -> Vec<String> {
    payload["visible_projects"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item["namespace_code"].as_str().map(ToOwned::to_owned))
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

fn count_foreign_namespace_hits(payload: &Value, local_namespace: &str) -> usize {
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
            .filter(|item| !item_belongs_to_namespace(item, local_namespace))
            .count()
    })
    .sum()
}

fn item_belongs_to_project(item: &Value, project: &str) -> bool {
    item["project_code"].as_str() == Some(project)
        || item["provenance"]["source_project"].as_str() == Some(project)
}

fn item_belongs_to_namespace(item: &Value, namespace: &str) -> bool {
    item["namespace_code"].as_str() == Some(namespace)
        || item["provenance"]["namespace_code"].as_str() == Some(namespace)
}

fn expected_project(item: &Value, expected_projects: &HashSet<&str>) -> bool {
    item["project_code"]
        .as_str()
        .or_else(|| item["provenance"]["source_project"].as_str())
        .is_some_and(|project| expected_projects.contains(project))
}

fn build_accuracy_eval_probes(
    args: &VerifyAccuracyArgs,
    suite: &AccuracySuiteManifest,
    symbol_pack: &retrieval::ContextPackResult,
    namespace_strict_pack: &retrieval::ContextPackResult,
    exact_precision: f64,
    lexical_precision: f64,
    semantic_precision: f64,
    cross_project_leakage: usize,
    strict_cross_namespace_leakage: usize,
    hostile_cross_project_leakage: usize,
    hostile_cross_namespace_leakage: usize,
    cross_namespace_leakage: usize,
    namespace_visible_projects_unexpected: usize,
) -> Result<Vec<AccuracyEvalProbe>> {
    let strict_boundary_clean = cross_project_leakage == 0 && strict_cross_namespace_leakage == 0;

    let related_expected_present =
        exact_precision > 0.0 || lexical_precision > 0.0 || semantic_precision > 0.0;
    let related_unexpected_present = (exact_precision > 0.0 && exact_precision < 1.0)
        || (lexical_precision > 0.0 && lexical_precision < 1.0)
        || (semantic_precision > 0.0 && semantic_precision < 1.0);

    let symbol_expected_present =
        any_matching_item(&symbol_pack.payload["retrieval"]["symbol_hits"], |item| {
            item["project_code"].as_str() == Some(args.project.as_str())
                && item["namespace_code"].as_str() == Some(args.namespace.as_str())
                && item["name"].as_str() == Some(suite.symbol_name.as_str())
        });
    let symbol_unexpected_present =
        any_non_matching_item(&symbol_pack.payload["retrieval"]["symbol_hits"], |item| {
            item["project_code"].as_str() == Some(args.project.as_str())
                && item["namespace_code"].as_str() == Some(args.namespace.as_str())
                && item["name"].as_str() == Some(suite.symbol_name.as_str())
        });

    let namespace_expected_present = payload_contains_text_hit(
        &namespace_strict_pack.payload,
        args.project.as_str(),
        suite.strict_namespace.as_str(),
        suite.namespace_query.as_str(),
    );
    let namespace_boundary_clean =
        cross_namespace_leakage == 0 && namespace_visible_projects_unexpected == 0;

    let hostile_boundary_clean =
        hostile_cross_project_leakage == 0 && hostile_cross_namespace_leakage == 0;

    Ok(vec![
        accuracy_eval_probe(
            "strict_local_fail_closed",
            EvalPattern::IsolationBoundary,
            EvalSignals {
                expected_present: Some(false),
                unexpected_present: !strict_boundary_clean,
                boundary_clean: Some(strict_boundary_clean),
                fail_closed_ok: Some(strict_boundary_clean),
                has_expected_target: false,
            },
            json!({
                "query": suite.strict_query,
                "project": args.project,
                "namespace": args.namespace,
                "boundary_clean": strict_boundary_clean,
                "cross_project_leakage": cross_project_leakage,
                "strict_cross_namespace_leakage": strict_cross_namespace_leakage,
            }),
        )?,
        accuracy_eval_probe(
            "related_retrieval_target",
            EvalPattern::RetrievalTarget,
            EvalSignals {
                expected_present: Some(related_expected_present),
                unexpected_present: related_unexpected_present,
                has_expected_target: true,
                ..EvalSignals::default()
            },
            json!({
                "query": suite.related_query,
                "related_path": suite.related_path,
                "related_term": suite.related_term,
                "expected_present": related_expected_present,
                "unexpected_present": related_unexpected_present,
            }),
        )?,
        accuracy_eval_probe(
            "symbol_target",
            EvalPattern::RetrievalTarget,
            EvalSignals {
                expected_present: Some(symbol_expected_present),
                unexpected_present: symbol_unexpected_present,
                has_expected_target: true,
                ..EvalSignals::default()
            },
            json!({
                "query": suite.namespace_query,
                "symbol_name": suite.symbol_name,
                "expected_present": symbol_expected_present,
                "unexpected_present": symbol_unexpected_present,
            }),
        )?,
        accuracy_eval_probe(
            "namespace_boundary",
            EvalPattern::IsolationBoundary,
            EvalSignals {
                expected_present: Some(namespace_expected_present),
                unexpected_present: !namespace_boundary_clean,
                boundary_clean: Some(namespace_boundary_clean),
                fail_closed_ok: Some(namespace_boundary_clean),
                has_expected_target: true,
            },
            json!({
                "query": suite.namespace_query,
                "namespace": suite.strict_namespace,
                "expected_present": namespace_expected_present,
                "boundary_clean": namespace_boundary_clean,
                "cross_namespace_leakage": cross_namespace_leakage,
            }),
        )?,
        accuracy_eval_probe(
            "hostile_fail_closed",
            EvalPattern::IsolationBoundary,
            EvalSignals {
                expected_present: Some(false),
                unexpected_present: !hostile_boundary_clean,
                boundary_clean: Some(hostile_boundary_clean),
                fail_closed_ok: Some(hostile_boundary_clean),
                has_expected_target: false,
            },
            json!({
                "query": suite.hostile_mixed_query,
                "boundary_clean": hostile_boundary_clean,
                "hostile_cross_project_leakage": hostile_cross_project_leakage,
                "hostile_cross_namespace_leakage": hostile_cross_namespace_leakage,
            }),
        )?,
    ])
}

fn accuracy_eval_probe(
    name: &'static str,
    pattern: EvalPattern,
    signals: EvalSignals,
    details: Value,
) -> Result<AccuracyEvalProbe> {
    let verdict = eval_verdict::derive_eval_verdict(pattern, &signals)?;
    Ok(AccuracyEvalProbe {
        name,
        verdict_class: verdict.class_key,
        verdict_reason: verdict.reason,
        details,
    })
}

fn build_accuracy_canonical_eval(probes: &[AccuracyEvalProbe]) -> Result<Value> {
    let mut summary = eval_verdict::summarize_eval_layer(
        probes.iter().map(|probe| probe.verdict_class.as_str()),
    )?;
    summary["probes"] = json!(
        probes
            .iter()
            .map(|probe| {
                json!({
                    "name": probe.name,
                    "eval_verdict_class": probe.verdict_class,
                    "eval_reason": probe.verdict_reason,
                    "details": probe.details,
                })
            })
            .collect::<Vec<_>>()
    );
    Ok(summary)
}

fn payload_contains_text_hit(
    payload: &Value,
    project: &str,
    namespace: &str,
    needle: &str,
) -> bool {
    if needle.is_empty() {
        return false;
    }
    any_matching_item(&payload["retrieval"]["exact_documents"], |item| {
        item_belongs_to_project(item, project)
            && item_belongs_to_namespace(item, namespace)
            && item["snippet"]
                .as_str()
                .is_some_and(|snippet| snippet.contains(needle))
    }) || any_matching_item(&payload["retrieval"]["lexical_chunks"], |item| {
        item_belongs_to_project(item, project)
            && item_belongs_to_namespace(item, namespace)
            && item["content"]
                .as_str()
                .is_some_and(|content| content.contains(needle))
    }) || any_matching_item(&payload["retrieval"]["semantic_chunks"], |item| {
        item_belongs_to_project(item, project)
            && item_belongs_to_namespace(item, namespace)
            && item["content"]
                .as_str()
                .is_some_and(|content| content.contains(needle))
    }) || any_matching_item(&payload["retrieval"]["symbol_hits"], |item| {
        item["project_code"].as_str() == Some(project)
            && item["namespace_code"].as_str() == Some(namespace)
            && item["name"]
                .as_str()
                .is_some_and(|name| name.contains(needle))
    })
}

fn any_matching_item(items: &Value, predicate: impl Fn(&Value) -> bool) -> bool {
    items.as_array().into_iter().flatten().any(predicate)
}

fn any_non_matching_item(items: &Value, predicate: impl Fn(&Value) -> bool) -> bool {
    items
        .as_array()
        .into_iter()
        .flatten()
        .any(|item| !predicate(item))
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
    use super::{
        AccuracyEvalProbe, StrategyOutcome, TextCompareCase, build_accuracy_canonical_eval,
        build_text_compare_canonical_eval, collect_visible_namespaces, count_foreign_hits,
        count_foreign_namespace_hits, item_belongs_to_namespace, item_belongs_to_project,
        item_matches_text_compare_case, normalize_engineering_context, payload_contains_text_hit,
        percentile_sample, precision_ratio, render_context_pack_prompt,
        render_filtered_context_prompt, safe_lossy_prefix, text_compare_eval_probe,
    };
    use crate::cli::ContextPackArgs;
    use proptest::prelude::*;
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
    fn normalize_engineering_context_rewrites_live_default() {
        let context = ContextPackArgs {
            project: "art".to_string(),
            namespace: "default".to_string(),
            query: "token drift".to_string(),
            retrieval_mode: None,
            disable_cache: false,
            limit_documents: 5,
            limit_symbols: 8,
            limit_chunks: 8,
            limit_semantic_chunks: 8,
            token_source_kind: "live_context_pack".to_string(),
            client_prompt_tokens: None,
            assistant_generation_tokens: None,
            tool_overhead_tokens: None,
            continuity_restore_tokens: None,
        };
        let normalized = normalize_engineering_context(&context, "benchmark_context_pack");
        assert_eq!(normalized.token_source_kind, "benchmark_context_pack");
    }

    #[test]
    fn normalize_engineering_context_preserves_explicit_non_live_source_kind() {
        let context = ContextPackArgs {
            project: "art".to_string(),
            namespace: "default".to_string(),
            query: "token drift".to_string(),
            retrieval_mode: None,
            disable_cache: false,
            limit_documents: 5,
            limit_symbols: 8,
            limit_chunks: 8,
            limit_semantic_chunks: 8,
            token_source_kind: "verify_context_pack".to_string(),
            client_prompt_tokens: None,
            assistant_generation_tokens: None,
            tool_overhead_tokens: None,
            continuity_restore_tokens: None,
        };
        let normalized = normalize_engineering_context(&context, "benchmark_context_pack");
        assert_eq!(normalized.token_source_kind, "verify_context_pack");
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
    fn payload_contains_text_hit_checks_project_namespace_and_content() {
        let payload = json!({
            "retrieval": {
                "exact_documents": [
                    {
                        "project_code": "alpha",
                        "namespace_code": "review",
                        "snippet": "needle token"
                    }
                ],
                "lexical_chunks": [],
                "semantic_chunks": [],
                "symbol_hits": []
            }
        });
        assert!(payload_contains_text_hit(
            &payload, "alpha", "review", "needle"
        ));
        assert!(!payload_contains_text_hit(
            &payload, "beta", "review", "needle"
        ));
    }

    #[test]
    fn canonical_eval_summary_keeps_probe_level_verdicts() {
        let summary = build_accuracy_canonical_eval(&[
            AccuracyEvalProbe {
                name: "strict_local_target",
                verdict_class: "hit_correct_target".to_string(),
                verdict_reason: "ok".to_string(),
                details: json!({"expected_present": true}),
            },
            AccuracyEvalProbe {
                name: "hostile_fail_closed",
                verdict_class: "hit_correct_target".to_string(),
                verdict_reason: "ok".to_string(),
                details: json!({"boundary_clean": true}),
            },
            AccuracyEvalProbe {
                name: "related_retrieval_target",
                verdict_class: "over_included".to_string(),
                verdict_reason: "noise".to_string(),
                details: json!({"unexpected_present": true}),
            },
        ])
        .expect("summary");
        assert_eq!(
            summary["verdict_counts"]["hit_correct_target"].as_u64(),
            Some(2)
        );
        assert_eq!(summary["verdict_counts"]["over_included"].as_u64(), Some(1));
        assert_eq!(
            summary["probes"][2]["name"],
            json!("related_retrieval_target")
        );
    }

    #[test]
    fn text_compare_eval_probe_marks_noise_as_over_included() {
        let case = TextCompareCase {
            query: "alpha_runtime_summary".to_string(),
            expected_projects: vec!["project_alpha".to_string()],
            expected_paths: vec!["src/lib.rs".to_string()],
            expected_terms: Vec::new(),
            expected_symbols: vec!["alpha_runtime_summary".to_string()],
            description: Some("alpha target".to_string()),
        };
        let probe = text_compare_eval_probe(
            "hybrid",
            &case,
            &StrategyOutcome {
                precision: 0.25,
                hit: true,
                head_hit: true,
                total_items: 4,
                matched_items: 1,
                prompt_tokens: 82,
            },
        )
        .expect("probe");
        assert_eq!(probe.verdict_class, "over_included");
        assert_eq!(probe.details["unexpected_present"].as_bool(), Some(true));
    }

    #[test]
    fn text_compare_eval_probe_marks_wrong_target_when_results_miss_expected() {
        let case = TextCompareCase {
            query: "alpha_runtime_summary".to_string(),
            expected_projects: vec!["project_alpha".to_string()],
            expected_paths: vec!["src/lib.rs".to_string()],
            expected_terms: Vec::new(),
            expected_symbols: vec!["alpha_runtime_summary".to_string()],
            description: None,
        };
        let probe = text_compare_eval_probe(
            "semantic_only",
            &case,
            &StrategyOutcome {
                precision: 0.0,
                hit: false,
                head_hit: false,
                total_items: 1,
                matched_items: 0,
                prompt_tokens: 67,
            },
        )
        .expect("probe");
        assert_eq!(probe.verdict_class, "hit_wrong_target");
    }

    #[test]
    fn text_compare_canonical_eval_keeps_strategy_breakdown() {
        let case = TextCompareCase {
            query: "shared_runtime_marker".to_string(),
            expected_projects: vec!["project_alpha".to_string(), "project_beta".to_string()],
            expected_paths: vec!["src/lib.rs".to_string()],
            expected_terms: vec!["shared_runtime_marker".to_string()],
            expected_symbols: Vec::new(),
            description: Some("shared token".to_string()),
        };
        let summary = build_text_compare_canonical_eval(&[
            text_compare_eval_probe(
                "hybrid",
                &case,
                &StrategyOutcome {
                    precision: 0.8,
                    hit: true,
                    head_hit: true,
                    total_items: 10,
                    matched_items: 8,
                    prompt_tokens: 223,
                },
            )
            .expect("hybrid probe"),
            text_compare_eval_probe(
                "semantic_only",
                &case,
                &StrategyOutcome {
                    precision: 1.0,
                    hit: true,
                    head_hit: true,
                    total_items: 2,
                    matched_items: 2,
                    prompt_tokens: 71,
                },
            )
            .expect("semantic probe"),
        ])
        .expect("summary");
        assert_eq!(summary["verdict_counts"]["over_included"].as_u64(), Some(1));
        assert_eq!(
            summary["strategy_breakdown"]["semantic_only"]["verdict_counts"]["hit_correct_target"]
                .as_u64(),
            Some(1)
        );
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

    #[test]
    fn item_belongs_to_namespace_checks_item_namespace() {
        assert!(item_belongs_to_namespace(
            &json!({"namespace_code":"default"}),
            "default"
        ));
        assert!(!item_belongs_to_namespace(
            &json!({"namespace_code":"review"}),
            "default"
        ));
    }

    #[test]
    fn safe_lossy_prefix_truncates_bytes() {
        let content = safe_lossy_prefix("abcdef".as_bytes(), 3);
        assert_eq!(content, "abc");
    }

    #[test]
    fn context_prompt_renders_sections() {
        let payload = json!({
            "query": "needle",
            "project": {"code": "alpha"},
            "effective_retrieval_mode": "local_strict",
            "retrieval": {
                "exact_documents": [{"project_code":"alpha","relative_path":"README.md","snippet":"needle"}],
                "symbol_hits": [{"provenance":{"source_project":"alpha"},"relative_path":"src/lib.rs","name":"run","kind":"fn"}],
                "lexical_chunks": [{"project_code":"alpha","relative_path":"src/lib.rs","content":"needle"}],
                "semantic_chunks": [{"provenance":{"source_project":"alpha"},"relative_path":"src/lib.rs","content":"needle"}]
            }
        });
        let prompt = render_context_pack_prompt(&payload);
        assert!(prompt.contains("Q:needle"));
        assert!(prompt.contains("D\n[alpha] README.md needle"));
        assert!(prompt.contains("S\n[alpha] src/lib.rs :: run :: fn"));
        assert!(prompt.contains("E\n[alpha] src/lib.rs :: needle"));
    }

    #[test]
    fn helper_collects_visible_namespaces_and_hits() {
        let payload = json!({
            "visible_projects": [
                {"project_code":"alpha","namespace_code":"default"},
                {"project_code":"beta","namespace_code":"review"}
            ],
            "retrieval": {
                "exact_documents": [{"project_code":"alpha","namespace_code":"default"}],
                "symbol_hits": [],
                "lexical_chunks": [{"project_code":"alpha","namespace_code":"default"}],
                "semantic_chunks": [{"project_code":"beta","namespace_code":"review"}]
            }
        });
        assert_eq!(
            collect_visible_namespaces(&payload),
            vec!["default".to_string(), "review".to_string()]
        );
        assert_eq!(count_foreign_namespace_hits(&payload, "default"), 1);
    }

    #[test]
    fn text_compare_case_matches_expected_project_path_and_term() {
        let case = TextCompareCase {
            query: "shared_runtime_marker".to_string(),
            expected_projects: vec!["project_alpha".to_string()],
            expected_paths: vec!["src/lib.rs".to_string()],
            expected_terms: vec!["shared_runtime_marker".to_string()],
            expected_symbols: Vec::new(),
            description: None,
        };
        assert!(item_matches_text_compare_case(
            &json!({
                "project_code":"project_alpha",
                "relative_path":"src/lib.rs",
                "content":"pub const SHARED_RUNTIME_MARKER: &str = \"shared_runtime_marker\";"
            }),
            &case
        ));
        assert!(!item_matches_text_compare_case(
            &json!({
                "project_code":"project_beta",
                "relative_path":"src/lib.rs",
                "content":"shared_runtime_marker"
            }),
            &case
        ));
    }

    #[test]
    fn filtered_prompt_can_hide_semantic_section() {
        let payload = json!({
            "query": "needle",
            "project": {"code": "alpha"},
            "effective_retrieval_mode": "local_strict",
            "retrieval": {
                "exact_documents": [],
                "symbol_hits": [],
                "lexical_chunks": [],
                "semantic_chunks": [{"provenance":{"source_project":"alpha"},"relative_path":"src/lib.rs","content":"needle"}]
            }
        });
        let prompt = render_filtered_context_prompt(&payload, false, false, false, false);
        assert!(!prompt.contains("[alpha] src/lib.rs :: needle"));
    }

    fn synthetic_payload(
        project_specs: &[(usize, u8)],
        namespace_specs: &[(usize, u8)],
    ) -> serde_json::Value {
        let mut exact_documents = Vec::new();
        let mut symbol_hits = Vec::new();
        let mut lexical_chunks = Vec::new();
        let mut semantic_chunks = Vec::new();
        for (section, project_case) in project_specs {
            let item = match project_case {
                0 => json!({"project_code":"alpha"}),
                1 => json!({"project_code":"beta"}),
                2 => json!({"provenance":{"source_project":"alpha"}}),
                _ => json!({"provenance":{"source_project":"beta"}}),
            };
            match section % 4 {
                0 => exact_documents.push(item),
                1 => symbol_hits.push(item),
                2 => lexical_chunks.push(item),
                _ => semantic_chunks.push(item),
            }
        }
        for (section, namespace_case) in namespace_specs {
            let item = match namespace_case {
                0 => json!({"namespace_code":"default"}),
                1 => json!({"namespace_code":"review"}),
                2 => json!({"provenance":{"namespace_code":"default"}}),
                _ => json!({"provenance":{"namespace_code":"review"}}),
            };
            match section % 4 {
                0 => exact_documents.push(item),
                1 => symbol_hits.push(item),
                2 => lexical_chunks.push(item),
                _ => semantic_chunks.push(item),
            }
        }
        json!({
            "retrieval": {
                "exact_documents": exact_documents,
                "symbol_hits": symbol_hits,
                "lexical_chunks": lexical_chunks,
                "semantic_chunks": semantic_chunks,
            }
        })
    }

    proptest! {
        #[test]
        fn foreign_project_hits_match_manual_fail_closed_count(
            specs in prop::collection::vec((0usize..4, 0u8..4), 0..32)
        ) {
            let payload = synthetic_payload(&specs, &[]);
            let expected = specs
                .iter()
                .filter(|(_, project_case)| !matches!(project_case, 0 | 2))
                .count();
            prop_assert_eq!(count_foreign_hits(&payload, "alpha"), expected);
        }

        #[test]
        fn foreign_namespace_hits_match_manual_fail_closed_count(
            specs in prop::collection::vec((0usize..4, 0u8..4), 0..32)
        ) {
            let payload = synthetic_payload(&[], &specs);
            let expected = specs
                .iter()
                .filter(|(_, namespace_case)| !matches!(namespace_case, 0 | 2))
                .count();
            prop_assert_eq!(count_foreign_namespace_hits(&payload, "default"), expected);
        }
    }
}
