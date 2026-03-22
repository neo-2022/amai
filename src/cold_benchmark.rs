use crate::cli::{ContextPackArgs, IndexProjectArgs, VerifyColdPathArgs};
use crate::config::AppConfig;
use crate::indexer;
use crate::postgres;
use crate::retrieval::{self, ContextPackStats};
use anyhow::{Context, Result, anyhow};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;
use sysinfo::{Disks, System};
use tokio::time::{Duration, sleep};
use tokio_postgres::Client;

#[derive(Debug, Clone, Deserialize)]
struct ColdBenchmarkManifest {
    profile: ColdBenchmarkProfile,
    repos: Vec<ColdBenchmarkRepo>,
    cases: Vec<ColdBenchmarkCase>,
}

#[derive(Debug, Clone, Deserialize)]
struct ColdBenchmarkProfile {
    display_name: String,
    summary: String,
    target_p95_ms: f64,
    target_p99_ms: f64,
    target_max_ms: f64,
    min_precision: f64,
    min_target_hit_rate: f64,
    min_recall: f64,
}

#[derive(Debug, Clone, Deserialize)]
struct ColdBenchmarkRepo {
    code: String,
    display_name: String,
    repo_root: PathBuf,
    namespace: String,
    repo_type: String,
    size_class: String,
    #[serde(default)]
    limit_files: Option<usize>,
    #[serde(default)]
    skip_embeddings: bool,
    #[serde(default = "default_local_strict")]
    default_retrieval_mode: String,
}

#[derive(Debug, Clone, Deserialize)]
struct ColdBenchmarkCase {
    repo_code: String,
    query_slice: String,
    query: String,
    #[serde(default)]
    retrieval_mode: Option<String>,
    #[serde(default)]
    expected_projects: Vec<String>,
    #[serde(default)]
    expected_paths: Vec<String>,
    #[serde(default)]
    expected_terms: Vec<String>,
    #[serde(default)]
    expected_symbols: Vec<String>,
}

#[derive(Debug, Clone)]
struct RepoRuntime {
    manifest: ColdBenchmarkRepo,
    resolved_root: PathBuf,
}

#[derive(Debug, Clone)]
struct QualityScore {
    precision: f64,
    recall: f64,
    target_hit: bool,
    head_hit: bool,
}

#[derive(Debug, Clone)]
struct RetrievalSample {
    cycle: usize,
    mode: &'static str,
    repo_code: String,
    repo_type: String,
    size_class: String,
    query_slice: String,
    query: String,
    total_ms: f64,
    policy_ms: f64,
    retrieval_ms: f64,
    ranking_ms: f64,
    provenance_ms: f64,
    pack_assembly_ms: f64,
    orchestration_ms: f64,
    precision: f64,
    recall: f64,
    target_hit: bool,
    miss: bool,
    fallback_triggered: bool,
    head_hit: bool,
}

#[derive(Debug, Clone)]
struct HardwareSample {
    cycle: usize,
    label: String,
    cpu_usage_pct: f64,
    available_memory_gib: f64,
    free_disk_gib: f64,
    max_temp_celsius: Option<f64>,
}

#[derive(Debug, Clone)]
struct IndexedRepoSummary {
    repo_code: String,
    display_name: String,
    repo_type: String,
    size_class: String,
    repo_root: String,
    files_indexed: usize,
    symbols_written: usize,
    chunks_written: usize,
    vector_points_written: usize,
    elapsed_ms: u128,
    parser_coverage_ratio: f64,
}

#[derive(Debug, Clone)]
struct CycleSummary {
    cycle: usize,
    cold: Distribution,
    hot: Distribution,
}

#[derive(Debug, Clone)]
struct SafetyEvent {
    kind: String,
    message: String,
}

#[derive(Debug, Clone)]
struct Distribution {
    p50: f64,
    p95: f64,
    p99: f64,
    max: f64,
    current: f64,
    sample_count: usize,
}

pub async fn run(cfg: &AppConfig, db: &mut Client, args: &VerifyColdPathArgs) -> Result<()> {
    if args.cycles == 0 {
        return Err(anyhow!("cold benchmark requires cycles > 0"));
    }

    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest_path = resolve_relative_path(repo_root, &args.manifest);
    let manifest = load_manifest(&manifest_path)?;
    if manifest.repos.is_empty() {
        return Err(anyhow!("cold benchmark manifest has no repos"));
    }
    if manifest.cases.is_empty() {
        return Err(anyhow!("cold benchmark manifest has no cases"));
    }

    let output_dir = resolve_relative_path(repo_root, &args.output_dir);
    let mut cleanup_actions_count = prepare_output_dir(&output_dir)?;
    let temp_dir = output_dir.join("tmp");
    fs::create_dir_all(&temp_dir)
        .with_context(|| format!("failed to create {}", temp_dir.display()))?;

    let hardware_profile = collect_hardware_profile(repo_root)?;
    let run_started = Instant::now();
    let mut hardware_samples = Vec::new();
    let mut indexed_repos = Vec::new();
    let mut cold_samples = Vec::new();
    let mut hot_samples = Vec::new();
    let mut cycle_summaries = Vec::new();
    let mut safety_events = Vec::new();
    let mut thermal_pause_count = 0usize;
    let mut thermal_stop_count = 0usize;
    let mut stop_reason = None::<String>;

    let repos = resolve_repos(repo_root, &manifest.repos)?;
    let repo_map = repos
        .iter()
        .map(|repo| (repo.manifest.code.clone(), repo.clone()))
        .collect::<BTreeMap<_, _>>();

    for repo in &repos {
        ensure_repo_registered(cfg, db, repo).await?;
        if !args.skip_index {
            let summary = index_repo(cfg, db, repo, true).await?;
            if let Some(summary) = summary {
                indexed_repos.push(summary);
            }
        }
    }

    'cycles: for cycle in 0..args.cycles {
        if args.reindex_each_cycle && !args.skip_index && cycle > 0 {
            for repo in repo_map.values() {
                if let Some(summary) = index_repo(cfg, db, repo, true).await? {
                    indexed_repos.push(summary);
                }
            }
        }
        let cold_start = cold_samples.len();
        let hot_start = hot_samples.len();
        for case in &manifest.cases {
            let repo = repo_map
                .get(&case.repo_code)
                .ok_or_else(|| anyhow!("unknown repo_code in manifest case: {}", case.repo_code))?;

            match enforce_safety_guards(
                repo_root,
                cycle,
                args,
                &mut hardware_samples,
                &mut thermal_pause_count,
                &mut thermal_stop_count,
                &mut safety_events,
            )
            .await?
            {
                GuardAction::Continue => {}
                GuardAction::Stop(reason) => {
                    stop_reason = Some(reason);
                    break 'cycles;
                }
            }

            let cold_case = run_case(cfg, db, repo, case, cycle, true).await?;
            let hot_case = run_case(cfg, db, repo, case, cycle, false).await?;
            cold_samples.push(cold_case.primary.clone());
            hot_samples.push(hot_case.primary.clone());

            if cold_case.fallback_triggered {
                let record = cold_samples
                    .last_mut()
                    .expect("cold sample exists after push");
                record.fallback_triggered = true;
            }
        }

        if cold_samples.len() > cold_start || hot_samples.len() > hot_start {
            cycle_summaries.push(CycleSummary {
                cycle,
                cold: distribution_from_samples(&cold_samples[cold_start..]),
                hot: distribution_from_samples(&hot_samples[hot_start..]),
            });
        }
    }

    cleanup_actions_count += cleanup_temp_dir(&temp_dir)?;

    let cold_distribution = distribution_from_samples(&cold_samples);
    let hot_distribution = distribution_from_samples(&hot_samples);
    let cold_quality = summarize_quality(&cold_samples);
    let hot_quality = summarize_quality(&hot_samples);
    let stage_breakdown = stage_breakdown(&cold_samples);
    let repo_coverage = repo_coverage(&repo_map, &cold_samples);
    let query_coverage = query_coverage(&cold_samples);
    let long_run = long_run_summary(&cycle_summaries);
    let hardware_summary = summarize_hardware(&hardware_samples, repo_root)?;
    let verdict = determine_verdict(
        &manifest.profile,
        &cold_distribution,
        &cold_quality,
        stop_reason.as_ref(),
    );
    let target_met = verdict == "TARGET MET";
    let duration_seconds = run_started.elapsed().as_secs_f64();

    let summary = json!({
        "cold_benchmark": {
            "profile": {
                "display_name": manifest.profile.display_name,
                "summary": manifest.profile.summary,
                "target_p95_ms": manifest.profile.target_p95_ms,
                "target_p99_ms": manifest.profile.target_p99_ms,
                "target_max_ms": manifest.profile.target_max_ms,
                "min_precision": manifest.profile.min_precision,
                "min_target_hit_rate": manifest.profile.min_target_hit_rate,
                "min_recall": manifest.profile.min_recall,
            },
            "executive_summary": {
                "verdict": verdict,
                "why": verdict_reason(&manifest.profile, &cold_distribution, &cold_quality, stop_reason.as_ref()),
                "cold_targets": {
                    "p95_le_10_ms": cold_distribution.p95 <= manifest.profile.target_p95_ms,
                    "p99_le_15_ms": cold_distribution.p99 <= manifest.profile.target_p99_ms,
                    "max_le_20_ms": cold_distribution.max <= manifest.profile.target_max_ms,
                },
                "sample_size": cold_distribution.sample_count,
                "repo_types": repo_coverage["repo_types"].clone(),
                "query_slices": query_coverage["query_slices"].clone(),
                "quality": cold_quality.clone(),
                "thermal_limited": thermal_pause_count > 0 || thermal_stop_count > 0,
                "disk_limited": stop_reason.as_ref().is_some_and(|reason| reason.contains("disk")),
            },
            "hardware_profile": hardware_profile,
            "dataset_coverage": repo_coverage,
            "query_coverage": query_coverage,
            "cold_latency_distribution": distribution_to_json(&cold_distribution),
            "hot_latency_distribution": distribution_to_json(&hot_distribution),
            "quality_metrics": {
                "cold": cold_quality.clone(),
                "hot": hot_quality,
            },
            "long_run_stability": {
                "cycles": cycle_summaries.iter().map(cycle_summary_to_json).collect::<Vec<_>>(),
                "summary": long_run,
            },
            "disk_cleanup_behavior": {
                "cleanup_actions_count": cleanup_actions_count,
                "min_disk_free_gib": hardware_summary["min_free_disk_gib"].clone(),
                "disk_growth_gib": hardware_summary["disk_growth_gib"].clone(),
            },
            "thermal_behavior": {
                "max_temperature_celsius": hardware_summary["max_temperature_celsius"].clone(),
                "thermal_pause_count": thermal_pause_count,
                "thermal_stop_count": thermal_stop_count,
                "stop_reason": stop_reason,
                "events": safety_events.iter().map(safety_event_to_json).collect::<Vec<_>>(),
            },
            "bottleneck_breakdown": stage_breakdown,
            "targets": {
                "cold_p95_ms": cold_distribution.p95,
                "cold_p99_ms": cold_distribution.p99,
                "cold_max_ms": cold_distribution.max,
                "target_p95_ms": manifest.profile.target_p95_ms,
                "target_p99_ms": manifest.profile.target_p99_ms,
                "target_max_ms": manifest.profile.target_max_ms,
                "target_met": target_met,
            },
            "machine_readable_summary": {
                "p50": cold_distribution.p50,
                "p95": cold_distribution.p95,
                "p99": cold_distribution.p99,
                "max": cold_distribution.max,
                "sample_count": cold_distribution.sample_count,
                "recall": cold_quality["recall"].as_f64().unwrap_or_default(),
                "precision": cold_quality["precision"].as_f64().unwrap_or_default(),
                "hit_rate": cold_quality["target_hit_rate"].as_f64().unwrap_or_default(),
                "miss_rate": cold_quality["miss_rate"].as_f64().unwrap_or_default(),
                "fallback_rate": cold_quality["fallback_rate"].as_f64().unwrap_or_default(),
                "repo_count": repo_map.len(),
                "query_slice_count": query_coverage["query_slice_count"].as_u64().unwrap_or_default(),
                "duration": duration_seconds,
                "target_met": target_met,
                "thermal_stop_count": thermal_stop_count,
                "cleanup_actions_count": cleanup_actions_count,
            },
            "indexed_repos": indexed_repos.iter().map(indexed_repo_to_json).collect::<Vec<_>>(),
        }
    });

    write_outputs(&output_dir, &summary, &cold_samples, &hot_samples)?;
    let _ = postgres::insert_observability_snapshot(db, "cold_path_benchmark", &summary).await?;
    println!("{}", serde_json::to_string_pretty(&summary)?);
    Ok(())
}

fn default_local_strict() -> String {
    "local_strict".to_string()
}

fn resolve_relative_path(repo_root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        repo_root.join(path)
    }
}

fn load_manifest(path: &Path) -> Result<ColdBenchmarkManifest> {
    let content =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    toml::from_str(&content).context("failed to parse cold benchmark manifest")
}

fn resolve_repos(repo_root: &Path, repos: &[ColdBenchmarkRepo]) -> Result<Vec<RepoRuntime>> {
    repos
        .iter()
        .map(|repo| {
            let resolved_root = resolve_relative_path(repo_root, &repo.repo_root);
            if !resolved_root.exists() {
                return Err(anyhow!(
                    "cold benchmark repo_root does not exist: {}",
                    resolved_root.display()
                ));
            }
            Ok(RepoRuntime {
                manifest: repo.clone(),
                resolved_root,
            })
        })
        .collect()
}

async fn ensure_repo_registered(
    cfg: &AppConfig,
    db: &mut Client,
    repo: &RepoRuntime,
) -> Result<()> {
    let project = postgres::upsert_project(
        db,
        &repo.manifest.code,
        &repo.manifest.display_name,
        &repo.resolved_root.display().to_string(),
        None,
        &cfg.default_retrieval_mode,
    )
    .await?;
    let _ = postgres::ensure_namespace(
        db,
        project.project_id,
        &repo.manifest.namespace,
        Some(&repo.manifest.namespace),
        &repo.manifest.default_retrieval_mode,
    )
    .await?;
    Ok(())
}

async fn index_repo(
    cfg: &AppConfig,
    db: &mut Client,
    repo: &RepoRuntime,
    run_index: bool,
) -> Result<Option<IndexedRepoSummary>> {
    if !run_index {
        return Ok(None);
    }
    let stats = indexer::index_project(
        cfg,
        db,
        &IndexProjectArgs {
            code: repo.manifest.code.clone(),
            path: repo.resolved_root.clone(),
            namespace: repo.manifest.namespace.clone(),
            limit_files: repo.manifest.limit_files,
            skip_embeddings: repo.manifest.skip_embeddings,
        },
    )
    .await?;
    Ok(Some(IndexedRepoSummary {
        repo_code: repo.manifest.code.clone(),
        display_name: repo.manifest.display_name.clone(),
        repo_type: repo.manifest.repo_type.clone(),
        size_class: repo.manifest.size_class.clone(),
        repo_root: repo.resolved_root.display().to_string(),
        files_indexed: stats.files_indexed,
        symbols_written: stats.symbols_written,
        chunks_written: stats.chunks_written,
        vector_points_written: stats.vector_points_written,
        elapsed_ms: stats.elapsed_ms,
        parser_coverage_ratio: stats.parser_coverage_ratio,
    }))
}

async fn run_case(
    cfg: &AppConfig,
    db: &mut Client,
    repo: &RepoRuntime,
    case: &ColdBenchmarkCase,
    cycle: usize,
    cold: bool,
) -> Result<CaseExecution> {
    let retrieval_mode = case
        .retrieval_mode
        .clone()
        .unwrap_or_else(|| repo.manifest.default_retrieval_mode.clone());
    let args = ContextPackArgs {
        project: repo.manifest.code.clone(),
        namespace: repo.manifest.namespace.clone(),
        query: case.query.clone(),
        retrieval_mode: Some(retrieval_mode.clone()),
        disable_cache: cold,
        limit_documents: 4,
        limit_symbols: 4,
        limit_chunks: 4,
        limit_semantic_chunks: 4,
    };
    let started = Instant::now();
    let pack =
        retrieval::execute_context_pack_capture_with_options(cfg, db, &args, false, false).await?;
    let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
    let quality = evaluate_case(&pack.payload, case);
    let mut fallback_triggered = false;
    if cold && !quality.target_hit {
        fallback_triggered = true;
        let fallback_mode = fallback_mode(&retrieval_mode);
        let fallback_args = ContextPackArgs {
            retrieval_mode: Some(fallback_mode),
            limit_documents: 8,
            limit_symbols: 8,
            limit_chunks: 8,
            limit_semantic_chunks: 8,
            ..args.clone()
        };
        let _ = retrieval::execute_context_pack_capture_with_options(
            cfg,
            db,
            &fallback_args,
            false,
            false,
        )
        .await?;
    }

    Ok(CaseExecution {
        primary: sample_from_case(repo, case, cycle, cold, elapsed_ms, &pack.stats, &quality),
        fallback_triggered,
    })
}

#[derive(Debug, Clone)]
struct CaseExecution {
    primary: RetrievalSample,
    fallback_triggered: bool,
}

fn sample_from_case(
    repo: &RepoRuntime,
    case: &ColdBenchmarkCase,
    cycle: usize,
    cold: bool,
    total_ms: f64,
    stats: &ContextPackStats,
    quality: &QualityScore,
) -> RetrievalSample {
    RetrievalSample {
        cycle,
        mode: if cold { "cold" } else { "hot" },
        repo_code: repo.manifest.code.clone(),
        repo_type: repo.manifest.repo_type.clone(),
        size_class: repo.manifest.size_class.clone(),
        query_slice: case.query_slice.clone(),
        query: case.query.clone(),
        total_ms,
        policy_ms: (stats.timings.resolve_scope_ms + stats.timings.cache_lookup_ms) as f64,
        retrieval_ms: (stats.timings.exact_lookup_ms
            + stats.timings.symbol_lookup_ms
            + stats.timings.lexical_lookup_ms
            + stats.timings.query_embed_ms
            + stats.timings.semantic_search_ms
            + stats.timings.semantic_hydrate_ms) as f64,
        ranking_ms: stats.timings.ranking_ms as f64,
        provenance_ms: stats.timings.provenance_ms as f64,
        pack_assembly_ms: (stats.timings.pack_assembly_ms + stats.timings.serialize_ms) as f64,
        orchestration_ms: total_ms,
        precision: quality.precision,
        recall: quality.recall,
        target_hit: quality.target_hit,
        miss: !quality.target_hit,
        fallback_triggered: false,
        head_hit: quality.head_hit,
    }
}

fn evaluate_case(payload: &Value, case: &ColdBenchmarkCase) -> QualityScore {
    let items = collect_strategy_items(payload);
    let total_items = items.len();
    let matched_items = items
        .iter()
        .filter(|item| item_matches_case(item, case))
        .count();
    let precision = if total_items == 0 {
        0.0
    } else {
        matched_items as f64 / total_items as f64
    };
    let head_hit = items
        .first()
        .map(|item| item_matches_case(item, case))
        .unwrap_or(false);
    let target_hit = matched_items > 0;

    let mut matched_targets = 0usize;
    let mut total_targets = 0usize;
    for expected in &case.expected_projects {
        total_targets += 1;
        if items
            .iter()
            .any(|item| item["project_code"].as_str() == Some(expected.as_str()))
        {
            matched_targets += 1;
        }
    }
    for expected in &case.expected_paths {
        total_targets += 1;
        if items
            .iter()
            .any(|item| item["relative_path"].as_str() == Some(expected.as_str()))
        {
            matched_targets += 1;
        }
    }
    for expected in &case.expected_symbols {
        total_targets += 1;
        if items
            .iter()
            .any(|item| item["name"].as_str() == Some(expected.as_str()))
        {
            matched_targets += 1;
        }
    }
    for expected in &case.expected_terms {
        total_targets += 1;
        if items.iter().any(|item| {
            item["snippet"]
                .as_str()
                .is_some_and(|text| text.contains(expected))
                || item["content"]
                    .as_str()
                    .is_some_and(|text| text.contains(expected))
        }) {
            matched_targets += 1;
        }
    }
    let recall = if total_targets == 0 {
        target_hit as usize as f64
    } else {
        matched_targets as f64 / total_targets as f64
    };

    QualityScore {
        precision,
        recall,
        target_hit,
        head_hit,
    }
}

fn collect_strategy_items(payload: &Value) -> Vec<Value> {
    let mut items = Vec::new();
    for key in [
        "exact_documents",
        "symbol_hits",
        "lexical_chunks",
        "semantic_chunks",
    ] {
        if let Some(array) = payload["retrieval"][key].as_array() {
            items.extend(array.iter().cloned());
        }
    }
    items
}

fn item_matches_case(item: &Value, case: &ColdBenchmarkCase) -> bool {
    if !case.expected_projects.is_empty()
        && item["project_code"].as_str().is_some_and(|value| {
            case.expected_projects
                .iter()
                .any(|expected| expected == value)
        })
    {
        return true;
    }
    if !case.expected_paths.is_empty()
        && item["relative_path"]
            .as_str()
            .is_some_and(|value| case.expected_paths.iter().any(|expected| expected == value))
    {
        return true;
    }
    if !case.expected_symbols.is_empty()
        && item["name"].as_str().is_some_and(|value| {
            case.expected_symbols
                .iter()
                .any(|expected| expected == value)
        })
    {
        return true;
    }
    if !case.expected_terms.is_empty()
        && case.expected_terms.iter().any(|term| {
            item["snippet"]
                .as_str()
                .is_some_and(|value| value.contains(term))
                || item["content"]
                    .as_str()
                    .is_some_and(|value| value.contains(term))
        })
    {
        return true;
    }
    false
}

fn fallback_mode(primary_mode: &str) -> String {
    let _ = primary_mode;
    "local_plus_related".to_string()
}

enum GuardAction {
    Continue,
    Stop(String),
}

async fn enforce_safety_guards(
    repo_root: &Path,
    cycle: usize,
    args: &VerifyColdPathArgs,
    hardware_samples: &mut Vec<HardwareSample>,
    thermal_pause_count: &mut usize,
    thermal_stop_count: &mut usize,
    safety_events: &mut Vec<SafetyEvent>,
) -> Result<GuardAction> {
    for attempt in 0..=args.max_cooldown_retries {
        let sample = collect_hardware_sample(repo_root, cycle, format!("guard:{attempt}"))?;
        hardware_samples.push(sample.clone());
        if sample.free_disk_gib < args.min_disk_free_gib {
            let reason = format!(
                "disk guard tripped: free {:.2} GiB below {:.2} GiB",
                sample.free_disk_gib, args.min_disk_free_gib
            );
            safety_events.push(SafetyEvent {
                kind: "disk_stop".to_string(),
                message: reason.clone(),
            });
            return Ok(GuardAction::Stop(reason));
        }
        if let Some(temp) = sample.max_temp_celsius
            && temp > args.thermal_guard_celsius
        {
            if attempt == args.max_cooldown_retries {
                *thermal_stop_count += 1;
                let reason = format!(
                    "thermal guard tripped: {:.1}C above {:.1}C after {} retries",
                    temp, args.thermal_guard_celsius, args.max_cooldown_retries
                );
                safety_events.push(SafetyEvent {
                    kind: "thermal_stop".to_string(),
                    message: reason.clone(),
                });
                return Ok(GuardAction::Stop(reason));
            }
            *thermal_pause_count += 1;
            safety_events.push(SafetyEvent {
                kind: "thermal_pause".to_string(),
                message: format!(
                    "thermal guard {:.1}C above {:.1}C, cooldown {}s",
                    temp, args.thermal_guard_celsius, args.cooldown_seconds
                ),
            });
            sleep(Duration::from_secs(args.cooldown_seconds)).await;
            continue;
        }
        return Ok(GuardAction::Continue);
    }
    Ok(GuardAction::Continue)
}

fn collect_hardware_profile(repo_root: &Path) -> Result<Value> {
    let mut system = System::new_all();
    system.refresh_memory();
    let disks = Disks::new_with_refreshed_list();
    Ok(json!({
        "cpu_model": system
            .cpus()
            .first()
            .map(|cpu| cpu.brand().trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "модель CPU не определена".to_string()),
        "logical_cpus": system.cpus().len(),
        "total_memory_gib": bytes_to_gib(system.total_memory()),
        "available_memory_gib": bytes_to_gib(system.available_memory()),
        "available_disk_gib": disk_available_for_path(&disks, repo_root)
            .map(bytes_to_gib)
            .unwrap_or_default(),
        "memory_type": detect_memory_type().unwrap_or_else(|| "не определено автоматически".to_string()),
    }))
}

fn collect_hardware_sample(
    repo_root: &Path,
    cycle: usize,
    label: String,
) -> Result<HardwareSample> {
    let mut system = System::new_all();
    system.refresh_cpu_usage();
    system.refresh_memory();
    let cpu_usage_pct = if system.cpus().is_empty() {
        0.0
    } else {
        system
            .cpus()
            .iter()
            .map(|cpu| cpu.cpu_usage() as f64)
            .sum::<f64>()
            / system.cpus().len() as f64
    };
    let disks = Disks::new_with_refreshed_list();
    Ok(HardwareSample {
        cycle,
        label,
        cpu_usage_pct,
        available_memory_gib: bytes_to_gib(system.available_memory()),
        free_disk_gib: disk_available_for_path(&disks, repo_root)
            .map(bytes_to_gib)
            .unwrap_or_default(),
        max_temp_celsius: read_max_temperature_celsius(),
    })
}

fn summarize_hardware(samples: &[HardwareSample], repo_root: &Path) -> Result<Value> {
    let disks = Disks::new_with_refreshed_list();
    let free_start = samples
        .first()
        .map(|sample| sample.free_disk_gib)
        .unwrap_or_else(|| {
            disk_available_for_path(&disks, repo_root)
                .map(bytes_to_gib)
                .unwrap_or_default()
        });
    let free_end = samples
        .last()
        .map(|sample| sample.free_disk_gib)
        .unwrap_or(free_start);
    let max_temp = samples
        .iter()
        .filter_map(|sample| sample.max_temp_celsius)
        .max_by(f64::total_cmp);
    let max_cpu = samples
        .iter()
        .map(|sample| sample.cpu_usage_pct)
        .max_by(f64::total_cmp)
        .unwrap_or_default();
    let min_mem = samples
        .iter()
        .map(|sample| sample.available_memory_gib)
        .min_by(f64::total_cmp)
        .unwrap_or_default();
    let min_disk = samples
        .iter()
        .map(|sample| sample.free_disk_gib)
        .min_by(f64::total_cmp)
        .unwrap_or(free_start);
    Ok(json!({
        "samples": samples.iter().map(hardware_sample_to_json).collect::<Vec<_>>(),
        "max_cpu_usage_pct": max_cpu,
        "min_available_memory_gib": min_mem,
        "min_free_disk_gib": min_disk,
        "disk_growth_gib": free_start - free_end,
        "max_temperature_celsius": max_temp,
    }))
}

fn distribution_from_samples(samples: &[RetrievalSample]) -> Distribution {
    if samples.is_empty() {
        return Distribution {
            p50: 0.0,
            p95: 0.0,
            p99: 0.0,
            max: 0.0,
            current: 0.0,
            sample_count: 0,
        };
    }
    let mut values = samples
        .iter()
        .map(|sample| sample.total_ms)
        .collect::<Vec<_>>();
    values.sort_by(f64::total_cmp);
    Distribution {
        p50: percentile_f64(&values, 50),
        p95: percentile_f64(&values, 95),
        p99: percentile_f64(&values, 99),
        max: *values.last().unwrap_or(&0.0),
        current: samples
            .last()
            .map(|sample| sample.total_ms)
            .unwrap_or_default(),
        sample_count: samples.len(),
    }
}

fn summarize_quality(samples: &[RetrievalSample]) -> Value {
    if samples.is_empty() {
        return json!({
            "precision": 0.0,
            "recall": 0.0,
            "target_hit_rate": 0.0,
            "miss_rate": 0.0,
            "fallback_rate": 0.0,
            "head_hit_rate": 0.0,
            "sample_count": 0,
        });
    }
    let precision =
        samples.iter().map(|sample| sample.precision).sum::<f64>() / samples.len() as f64;
    let recall = samples.iter().map(|sample| sample.recall).sum::<f64>() / samples.len() as f64;
    let target_hit_rate = samples
        .iter()
        .map(|sample| sample.target_hit as usize as f64)
        .sum::<f64>()
        / samples.len() as f64;
    let fallback_rate = samples
        .iter()
        .map(|sample| sample.fallback_triggered as usize as f64)
        .sum::<f64>()
        / samples.len() as f64;
    let head_hit_rate = samples
        .iter()
        .map(|sample| sample.head_hit as usize as f64)
        .sum::<f64>()
        / samples.len() as f64;
    json!({
        "precision": precision,
        "recall": recall,
        "target_hit_rate": target_hit_rate,
        "miss_rate": 1.0 - target_hit_rate,
        "fallback_rate": fallback_rate,
        "head_hit_rate": head_hit_rate,
        "sample_count": samples.len(),
    })
}

fn stage_breakdown(samples: &[RetrievalSample]) -> Value {
    let mut groups = BTreeMap::<&str, Vec<f64>>::new();
    for sample in samples {
        groups.entry("policy").or_default().push(sample.policy_ms);
        groups
            .entry("retrieval")
            .or_default()
            .push(sample.retrieval_ms);
        groups.entry("ranking").or_default().push(sample.ranking_ms);
        groups
            .entry("provenance")
            .or_default()
            .push(sample.provenance_ms);
        groups
            .entry("pack_assembly")
            .or_default()
            .push(sample.pack_assembly_ms);
        groups
            .entry("orchestration")
            .or_default()
            .push(sample.orchestration_ms);
    }
    let mut result = serde_json::Map::new();
    for (group, values) in groups {
        let distribution = distribution_from_f64(&values);
        result.insert(group.to_string(), distribution_to_json(&distribution));
    }
    Value::Object(result)
}

fn repo_coverage(repo_map: &BTreeMap<String, RepoRuntime>, samples: &[RetrievalSample]) -> Value {
    let repo_types = repo_map
        .values()
        .map(|repo| repo.manifest.repo_type.clone())
        .collect::<BTreeSet<_>>();
    let size_classes = repo_map
        .values()
        .map(|repo| repo.manifest.size_class.clone())
        .collect::<BTreeSet<_>>();
    json!({
        "repo_count": repo_map.len(),
        "repo_types": repo_types,
        "size_classes": size_classes,
        "repos_with_queries": samples
            .iter()
            .map(|sample| sample.repo_code.clone())
            .collect::<BTreeSet<_>>(),
    })
}

fn query_coverage(samples: &[RetrievalSample]) -> Value {
    let mut per_slice = BTreeMap::<String, Vec<RetrievalSample>>::new();
    for sample in samples {
        per_slice
            .entry(sample.query_slice.clone())
            .or_default()
            .push(sample.clone());
    }
    json!({
        "query_slice_count": per_slice.len(),
        "query_slices": per_slice.keys().cloned().collect::<Vec<_>>(),
        "per_slice": per_slice
            .into_iter()
            .map(|(slice, slice_samples)| {
                (
                    slice,
                    json!({
                        "sample_count": slice_samples.len(),
                        "cold_latency": distribution_to_json(&distribution_from_samples(&slice_samples)),
                        "quality": summarize_quality(&slice_samples),
                    }),
                )
            })
            .collect::<serde_json::Map<_, _>>(),
    })
}

fn long_run_summary(cycles: &[CycleSummary]) -> Value {
    if cycles.len() < 2 {
        return json!({
            "cycles_count": cycles.len(),
            "cold_p95_drift_ms": 0.0,
            "hot_p95_drift_ms": 0.0,
            "degradation_detected": false,
        });
    }
    let first = cycles.first().expect("first cycle exists");
    let last = cycles.last().expect("last cycle exists");
    let cold_drift = last.cold.p95 - first.cold.p95;
    let hot_drift = last.hot.p95 - first.hot.p95;
    json!({
        "cycles_count": cycles.len(),
        "cold_p95_drift_ms": cold_drift,
        "hot_p95_drift_ms": hot_drift,
        "degradation_detected": cold_drift > 1.0 || hot_drift > 1.0,
    })
}

fn determine_verdict(
    profile: &ColdBenchmarkProfile,
    cold_distribution: &Distribution,
    cold_quality: &Value,
    stop_reason: Option<&String>,
) -> &'static str {
    if stop_reason.is_some() {
        return "NOT MET";
    }
    let precision = cold_quality["precision"].as_f64().unwrap_or_default();
    let recall = cold_quality["recall"].as_f64().unwrap_or_default();
    let hit_rate = cold_quality["target_hit_rate"].as_f64().unwrap_or_default();
    let latency_met = cold_distribution.p95 <= profile.target_p95_ms
        && cold_distribution.p99 <= profile.target_p99_ms
        && cold_distribution.max <= profile.target_max_ms;
    let quality_met = precision >= profile.min_precision
        && recall >= profile.min_recall
        && hit_rate >= profile.min_target_hit_rate;
    if latency_met && quality_met {
        "TARGET MET"
    } else if quality_met && cold_distribution.sample_count > 0 {
        "PARTIALLY MET"
    } else {
        "NOT MET"
    }
}

fn verdict_reason(
    profile: &ColdBenchmarkProfile,
    cold_distribution: &Distribution,
    cold_quality: &Value,
    stop_reason: Option<&String>,
) -> String {
    if let Some(reason) = stop_reason {
        return format!("Run stopped by safety guard: {reason}");
    }
    let precision = cold_quality["precision"].as_f64().unwrap_or_default();
    let recall = cold_quality["recall"].as_f64().unwrap_or_default();
    let hit_rate = cold_quality["target_hit_rate"].as_f64().unwrap_or_default();
    let mut reasons = Vec::new();
    if cold_distribution.p95 > profile.target_p95_ms {
        reasons.push(format!(
            "cold p95 {:.3} ms выше целевого {:.3} ms",
            cold_distribution.p95, profile.target_p95_ms
        ));
    }
    if cold_distribution.p99 > profile.target_p99_ms {
        reasons.push(format!(
            "cold p99 {:.3} ms выше целевого {:.3} ms",
            cold_distribution.p99, profile.target_p99_ms
        ));
    }
    if cold_distribution.max > profile.target_max_ms {
        reasons.push(format!(
            "cold max {:.3} ms выше целевого {:.3} ms",
            cold_distribution.max, profile.target_max_ms
        ));
    }
    if precision < profile.min_precision {
        reasons.push(format!(
            "precision {:.3} ниже допустимого {:.3}",
            precision, profile.min_precision
        ));
    }
    if recall < profile.min_recall {
        reasons.push(format!(
            "recall {:.3} ниже допустимого {:.3}",
            recall, profile.min_recall
        ));
    }
    if hit_rate < profile.min_target_hit_rate {
        reasons.push(format!(
            "target-hit rate {:.3} ниже допустимого {:.3}",
            hit_rate, profile.min_target_hit_rate
        ));
    }
    if reasons.is_empty() {
        "Все текущие latency и quality цели удержаны на этом прогоне.".to_string()
    } else {
        reasons.join("; ")
    }
}

fn distribution_from_f64(values: &[f64]) -> Distribution {
    if values.is_empty() {
        return Distribution {
            p50: 0.0,
            p95: 0.0,
            p99: 0.0,
            max: 0.0,
            current: 0.0,
            sample_count: 0,
        };
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    Distribution {
        p50: percentile_f64(&sorted, 50),
        p95: percentile_f64(&sorted, 95),
        p99: percentile_f64(&sorted, 99),
        max: *sorted.last().unwrap_or(&0.0),
        current: *values.last().unwrap_or(&0.0),
        sample_count: values.len(),
    }
}

fn percentile_f64(sorted: &[f64], percentile: u32) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let index = (((sorted.len() - 1) as f64) * percentile as f64 / 100.0).round() as usize;
    sorted[index.min(sorted.len() - 1)]
}

fn distribution_to_json(distribution: &Distribution) -> Value {
    json!({
        "current": distribution.current,
        "p50": distribution.p50,
        "p95": distribution.p95,
        "p99": distribution.p99,
        "max": distribution.max,
        "sample_count": distribution.sample_count,
    })
}

fn indexed_repo_to_json(summary: &IndexedRepoSummary) -> Value {
    json!({
        "repo_code": summary.repo_code,
        "display_name": summary.display_name,
        "repo_type": summary.repo_type,
        "size_class": summary.size_class,
        "repo_root": summary.repo_root,
        "files_indexed": summary.files_indexed,
        "symbols_written": summary.symbols_written,
        "chunks_written": summary.chunks_written,
        "vector_points_written": summary.vector_points_written,
        "elapsed_ms": summary.elapsed_ms,
        "parser_coverage_ratio": summary.parser_coverage_ratio,
    })
}

fn cycle_summary_to_json(summary: &CycleSummary) -> Value {
    json!({
        "cycle": summary.cycle,
        "cold": distribution_to_json(&summary.cold),
        "hot": distribution_to_json(&summary.hot),
    })
}

fn safety_event_to_json(event: &SafetyEvent) -> Value {
    json!({
        "kind": event.kind,
        "message": event.message,
    })
}

fn hardware_sample_to_json(sample: &HardwareSample) -> Value {
    json!({
        "cycle": sample.cycle,
        "label": sample.label,
        "cpu_usage_pct": sample.cpu_usage_pct,
        "available_memory_gib": sample.available_memory_gib,
        "free_disk_gib": sample.free_disk_gib,
        "max_temperature_celsius": sample.max_temp_celsius,
    })
}

fn write_outputs(
    output_dir: &Path,
    summary: &Value,
    cold_samples: &[RetrievalSample],
    hot_samples: &[RetrievalSample],
) -> Result<()> {
    fs::create_dir_all(output_dir)
        .with_context(|| format!("failed to create {}", output_dir.display()))?;
    let summary_path = output_dir.join("summary.json");
    fs::write(&summary_path, serde_json::to_string_pretty(summary)?)
        .with_context(|| format!("failed to write {}", summary_path.display()))?;

    let report_path = output_dir.join("report.md");
    fs::write(
        &report_path,
        render_report(summary, cold_samples.len(), hot_samples.len()),
    )
    .with_context(|| format!("failed to write {}", report_path.display()))?;

    let csv_path = output_dir.join("samples.csv");
    fs::write(&csv_path, render_samples_csv(cold_samples, hot_samples))
        .with_context(|| format!("failed to write {}", csv_path.display()))?;
    Ok(())
}

fn render_report(summary: &Value, cold_count: usize, hot_count: usize) -> String {
    let benchmark = &summary["cold_benchmark"];
    let executive = &benchmark["executive_summary"];
    format!(
        "# Cold Benchmark Report\n\n## Summary\n- Verdict: {}\n- Why: {}\n- Cold samples: {}\n- Hot shadow samples: {}\n\n## Hardware profile\n{}\n\n## Dataset coverage\n{}\n\n## Query coverage\n{}\n\n## Cold latency distribution\n{}\n\n## Hot latency distribution\n{}\n\n## Quality metrics\n{}\n\n## Long-run stability\n{}\n\n## Disk / cleanup behavior\n{}\n\n## Thermal behavior\n{}\n\n## Bottleneck breakdown\n{}\n\n## Did / did not meet targets\n{}\n\n## Next optimization steps\n- Сужать cold p95/p99/max без потери precision/recall/hit rate.\n- Сокращать хвосты по group breakdown, а не только средние значения.\n- Увеличивать размер real-repo cold set и длительность цикла без потери thermal safety.\n",
        executive["verdict"].as_str().unwrap_or("NOT MET"),
        executive["why"].as_str().unwrap_or("reason missing"),
        cold_count,
        hot_count,
        serde_json::to_string_pretty(&benchmark["hardware_profile"]).unwrap_or_default(),
        serde_json::to_string_pretty(&benchmark["dataset_coverage"]).unwrap_or_default(),
        serde_json::to_string_pretty(&benchmark["query_coverage"]).unwrap_or_default(),
        serde_json::to_string_pretty(&benchmark["cold_latency_distribution"]).unwrap_or_default(),
        serde_json::to_string_pretty(&benchmark["hot_latency_distribution"]).unwrap_or_default(),
        serde_json::to_string_pretty(&benchmark["quality_metrics"]).unwrap_or_default(),
        serde_json::to_string_pretty(&benchmark["long_run_stability"]).unwrap_or_default(),
        serde_json::to_string_pretty(&benchmark["disk_cleanup_behavior"]).unwrap_or_default(),
        serde_json::to_string_pretty(&benchmark["thermal_behavior"]).unwrap_or_default(),
        serde_json::to_string_pretty(&benchmark["bottleneck_breakdown"]).unwrap_or_default(),
        serde_json::to_string_pretty(&benchmark["targets"]).unwrap_or_default(),
    )
}

fn render_samples_csv(cold_samples: &[RetrievalSample], hot_samples: &[RetrievalSample]) -> String {
    let mut output = String::from(
        "mode,cycle,repo_code,repo_type,size_class,query_slice,query,total_ms,policy_ms,retrieval_ms,ranking_ms,provenance_ms,pack_assembly_ms,orchestration_ms,precision,recall,target_hit,miss,fallback_triggered,head_hit\n",
    );
    for sample in cold_samples.iter().chain(hot_samples.iter()) {
        output.push_str(&format!(
            "{},{},{},{},{},{},{:?},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.4},{:.4},{},{},{},{}\n",
            sample.mode,
            sample.cycle,
            sample.repo_code,
            sample.repo_type,
            sample.size_class,
            sample.query_slice,
            sample.query,
            sample.total_ms,
            sample.policy_ms,
            sample.retrieval_ms,
            sample.ranking_ms,
            sample.provenance_ms,
            sample.pack_assembly_ms,
            sample.orchestration_ms,
            sample.precision,
            sample.recall,
            sample.target_hit,
            sample.miss,
            sample.fallback_triggered,
            sample.head_hit,
        ));
    }
    output
}

fn prepare_output_dir(output_dir: &Path) -> Result<usize> {
    if output_dir.exists() {
        fs::remove_dir_all(output_dir)
            .with_context(|| format!("failed to clean {}", output_dir.display()))?;
        return Ok(1);
    }
    Ok(0)
}

fn cleanup_temp_dir(temp_dir: &Path) -> Result<usize> {
    if temp_dir.exists() {
        fs::remove_dir_all(temp_dir)
            .with_context(|| format!("failed to clean {}", temp_dir.display()))?;
        return Ok(1);
    }
    Ok(0)
}

fn bytes_to_gib(bytes: u64) -> f64 {
    bytes as f64 / 1024.0 / 1024.0 / 1024.0
}

fn disk_available_for_path(disks: &Disks, path: &Path) -> Option<u64> {
    disks
        .iter()
        .filter(|disk| path.starts_with(disk.mount_point()))
        .max_by_key(|disk| disk.mount_point().as_os_str().len())
        .map(|disk| disk.available_space())
}

fn detect_memory_type() -> Option<String> {
    for (program, args) in [
        ("dmidecode", vec!["--type", "17"]),
        ("lshw", vec!["-class", "memory"]),
    ] {
        let output = std::process::Command::new(program)
            .args(args)
            .output()
            .ok()?;
        if !output.status.success() {
            continue;
        }
        let text = String::from_utf8_lossy(&output.stdout);
        if let Some(found) = extract_memory_generation(&text) {
            return Some(found);
        }
    }
    None
}

fn extract_memory_generation(text: &str) -> Option<String> {
    let patterns = ["DDR5", "DDR4", "DDR3", "LPDDR5", "LPDDR4"];
    for line in text.lines() {
        for pattern in patterns {
            if line.to_ascii_uppercase().contains(pattern) {
                return Some(pattern.to_string());
            }
        }
    }
    None
}

fn read_max_temperature_celsius() -> Option<f64> {
    let thermal_root = Path::new("/sys/class/thermal");
    let entries = fs::read_dir(thermal_root).ok()?;
    let mut max_temp = None::<f64>;
    for entry in entries.flatten() {
        let path = entry.path().join("temp");
        let Ok(raw) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(value) = raw.trim().parse::<f64>() else {
            continue;
        };
        let celsius = if value > 1000.0 {
            value / 1000.0
        } else {
            value
        };
        max_temp = Some(match max_temp {
            Some(current) => current.max(celsius),
            None => celsius,
        });
    }
    max_temp
}

#[cfg(test)]
mod tests {
    use super::{ColdBenchmarkCase, determine_verdict, distribution_from_f64, evaluate_case};
    use crate::cold_benchmark::{ColdBenchmarkProfile, item_matches_case};
    use serde_json::json;

    #[test]
    fn evaluate_case_uses_expected_paths_terms_and_symbols() {
        let payload = json!({
            "retrieval": {
                "exact_documents": [
                    {
                        "project_code": "amai_local",
                        "relative_path": "src/verify.rs",
                        "name": "run_text_compare",
                        "snippet": "pub async fn run_text_compare("
                    }
                ],
                "symbol_hits": [],
                "lexical_chunks": [],
                "semantic_chunks": []
            }
        });
        let case = ColdBenchmarkCase {
            repo_code: "amai_local".to_string(),
            query_slice: "symbol_lookup".to_string(),
            query: "run_text_compare".to_string(),
            retrieval_mode: None,
            expected_projects: vec!["amai_local".to_string()],
            expected_paths: vec!["src/verify.rs".to_string()],
            expected_terms: vec!["run_text_compare".to_string()],
            expected_symbols: vec!["run_text_compare".to_string()],
        };
        let score = evaluate_case(&payload, &case);
        assert!(score.target_hit);
        assert_eq!(score.precision, 1.0);
        assert_eq!(score.recall, 1.0);
        assert!(score.head_hit);
    }

    #[test]
    fn distribution_from_f64_exposes_expected_percentiles() {
        let distribution = distribution_from_f64(&[1.0, 2.0, 3.0, 4.0, 5.0]);
        assert_eq!(distribution.p50, 3.0);
        assert_eq!(distribution.max, 5.0);
        assert_eq!(distribution.sample_count, 5);
    }

    #[test]
    fn determine_verdict_marks_latency_only_miss_as_partial() {
        let profile = ColdBenchmarkProfile {
            display_name: "test".to_string(),
            summary: "test".to_string(),
            target_p95_ms: 10.0,
            target_p99_ms: 15.0,
            target_max_ms: 20.0,
            min_precision: 0.99,
            min_target_hit_rate: 0.99,
            min_recall: 0.99,
        };
        let quality = json!({
            "precision": 1.0,
            "recall": 1.0,
            "target_hit_rate": 1.0,
        });
        let distribution = distribution_from_f64(&[5.0, 12.0, 16.0, 20.0]);
        assert_eq!(
            determine_verdict(&profile, &distribution, &quality, None),
            "PARTIALLY MET"
        );
    }

    #[test]
    fn item_matches_case_checks_multiple_evidence_types() {
        let item = json!({
            "project_code": "amai_local",
            "relative_path": "README.md",
            "name": "install_amai",
            "snippet": "install_amai.sh"
        });
        let case = ColdBenchmarkCase {
            repo_code: "amai_local".to_string(),
            query_slice: "onboarding_query".to_string(),
            query: "install_amai.sh".to_string(),
            retrieval_mode: None,
            expected_projects: vec!["amai_local".to_string()],
            expected_paths: vec!["README.md".to_string()],
            expected_terms: vec!["install_amai.sh".to_string()],
            expected_symbols: vec!["install_amai".to_string()],
        };
        assert!(item_matches_case(&item, &case));
    }
}
