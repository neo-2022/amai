use crate::cli::{ContextPackArgs, IndexProjectArgs, VerifyColdPathArgs};
use crate::config::AppConfig;
use crate::eval_verdict::{self, EvalPattern, EvalSignals};
use crate::indexer;
use crate::postgres;
use crate::retrieval::{self, ContextPackStats};
use crate::retrieval_science;
use anyhow::{Context, Result, anyhow};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
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
    target_p50_ms: f64,
    target_p95_ms: f64,
    target_p99_ms: f64,
    target_max_ms: f64,
    min_precision: f64,
    min_target_hit_rate: f64,
    min_recall: f64,
    min_sample_count: u64,
    min_repo_count: u64,
    min_query_slice_count: u64,
    max_duration_seconds: f64,
    max_leakage: u64,
    max_error_rate: f64,
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
    limit_documents: Option<usize>,
    #[serde(default)]
    limit_symbols: Option<usize>,
    #[serde(default)]
    limit_chunks: Option<usize>,
    #[serde(default)]
    limit_semantic_chunks: Option<usize>,
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
    leakage_count: u64,
    total_items: usize,
    matched_items: usize,
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
    leakage_count: u64,
    eval_verdict_class: String,
    eval_reason: String,
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

struct LiveProgressGuard {
    repo_root: PathBuf,
    output_dir: PathBuf,
}

impl LiveProgressGuard {
    fn new(repo_root: PathBuf, output_dir: PathBuf) -> Self {
        let guard = Self {
            repo_root,
            output_dir,
        };
        guard.clear();
        guard
    }

    fn write(&self, payload: &Value) -> Result<()> {
        let text = serde_json::to_string_pretty(payload)?;
        for path in [
            live_progress_cache_path(&self.repo_root),
            output_progress_path(&self.output_dir),
        ] {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("failed to create {}", parent.display()))?;
            }
            fs::write(&path, &text)
                .with_context(|| format!("failed to write {}", path.display()))?;
        }
        Ok(())
    }

    fn clear(&self) {
        for path in [
            live_progress_cache_path(&self.repo_root),
            output_progress_path(&self.output_dir),
        ] {
            let _ = fs::remove_file(path);
        }
    }
}

impl Drop for LiveProgressGuard {
    fn drop(&mut self) {
        self.clear();
    }
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
    let progress_guard = LiveProgressGuard::new(repo_root.to_path_buf(), output_dir.clone());

    let hardware_profile = collect_hardware_profile(repo_root)?;
    let run_started = Instant::now();
    let run_started_epoch_ms = now_epoch_ms();
    let mut hardware_samples = Vec::new();
    let mut indexed_repos = Vec::new();
    let mut cold_samples = Vec::new();
    let mut hot_samples = Vec::new();
    let mut cycle_summaries = Vec::new();
    let mut safety_events = Vec::new();
    let mut thermal_pause_count = 0usize;
    let mut thermal_stop_count = 0usize;
    let mut stop_reason = None::<String>;
    let mut cold_workload_elapsed_ms = 0.0f64;
    let mut indexed_repo_codes = BTreeSet::new();

    let repos = resolve_repos(repo_root, &manifest.repos)?;
    let repo_map = repos
        .iter()
        .map(|repo| (repo.manifest.code.clone(), repo.clone()))
        .collect::<BTreeMap<_, _>>();
    let repo_target_file_counts = repos
        .iter()
        .map(|repo| {
            let count =
                indexer::collect_files(&repo.resolved_root, repo.manifest.limit_files, None)?.len();
            Ok((repo.manifest.code.clone(), count))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    progress_guard.write(&build_live_progress_payload(
        &output_dir,
        &manifest,
        args,
        &repo_map,
        &repo_target_file_counts,
        run_started_epoch_ms,
        &cold_samples,
        &hot_samples,
        &cycle_summaries,
        &indexed_repos,
        "initializing",
        None,
        None,
        None,
    )?)?;

    for repo in &repos {
        ensure_repo_registered(cfg, db, repo).await?;
    }

    'cycles: for cycle in 0..args.cycles {
        let mut cycle_indexed_repo_codes = BTreeSet::new();
        let cold_start = cold_samples.len();
        let hot_start = hot_samples.len();
        for case in &manifest.cases {
            let repo = repo_map
                .get(&case.repo_code)
                .ok_or_else(|| anyhow!("unknown repo_code in manifest case: {}", case.repo_code))?;
            if should_index_repo_for_case(
                args.skip_index,
                args.reindex_each_cycle,
                &repo.manifest.code,
                &indexed_repo_codes,
                &cycle_indexed_repo_codes,
            ) {
                progress_guard.write(&build_live_progress_payload(
                    &output_dir,
                    &manifest,
                    args,
                    &repo_map,
                    &repo_target_file_counts,
                    run_started_epoch_ms,
                    &cold_samples,
                    &hot_samples,
                    &cycle_summaries,
                    &indexed_repos,
                    "indexing",
                    Some(cycle),
                    Some(&repo.manifest.code),
                    Some(&case.query_slice),
                )?)?;
                if let Some(summary) = index_repo(cfg, db, repo, true).await? {
                    indexed_repos.push(summary);
                    indexed_repo_codes.insert(repo.manifest.code.clone());
                    cycle_indexed_repo_codes.insert(repo.manifest.code.clone());
                }
            }
            progress_guard.write(&build_live_progress_payload(
                &output_dir,
                &manifest,
                args,
                &repo_map,
                &repo_target_file_counts,
                run_started_epoch_ms,
                &cold_samples,
                &hot_samples,
                &cycle_summaries,
                &indexed_repos,
                "running",
                Some(cycle),
                Some(&repo.manifest.code),
                Some(&case.query_slice),
            )?)?;

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
            cold_workload_elapsed_ms += cold_case.primary.total_ms + cold_case.fallback_elapsed_ms;
            cold_samples.push(cold_case.primary.clone());
            hot_samples.push(hot_case.primary.clone());

            if cold_case.fallback_triggered {
                let record = cold_samples
                    .last_mut()
                    .expect("cold sample exists after push");
                record.fallback_triggered = true;
            }
            progress_guard.write(&build_live_progress_payload(
                &output_dir,
                &manifest,
                args,
                &repo_map,
                &repo_target_file_counts,
                run_started_epoch_ms,
                &cold_samples,
                &hot_samples,
                &cycle_summaries,
                &indexed_repos,
                "running",
                Some(cycle),
                Some(&repo.manifest.code),
                Some(&case.query_slice),
            )?)?;
        }

        if cold_samples.len() > cold_start || hot_samples.len() > hot_start {
            cycle_summaries.push(CycleSummary {
                cycle,
                cold: distribution_from_samples(&cold_samples[cold_start..]),
                hot: distribution_from_samples(&hot_samples[hot_start..]),
            });
            progress_guard.write(&build_live_progress_payload(
                &output_dir,
                &manifest,
                args,
                &repo_map,
                &repo_target_file_counts,
                run_started_epoch_ms,
                &cold_samples,
                &hot_samples,
                &cycle_summaries,
                &indexed_repos,
                "cycle_complete",
                Some(cycle),
                None,
                None,
            )?)?;
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
    let duration_seconds = cold_workload_elapsed_ms / 1000.0;
    let run_wall_clock_duration_seconds = run_started.elapsed().as_secs_f64();
    let measured_repo_count = repo_coverage["repo_count"].as_u64().unwrap_or_default();
    let measured_query_slice_count = query_coverage["query_slice_count"]
        .as_u64()
        .unwrap_or_default();
    let verdict = determine_verdict(
        &manifest.profile,
        &cold_distribution,
        &cold_quality,
        measured_repo_count,
        measured_query_slice_count,
        duration_seconds,
        stop_reason.as_ref(),
    );
    let target_met = verdict == "TARGET MET";
    let canonical_eval = build_cold_benchmark_canonical_eval(&cold_samples)?;
    progress_guard.write(&build_live_progress_payload(
        &output_dir,
        &manifest,
        args,
        &repo_map,
        &repo_target_file_counts,
        run_started_epoch_ms,
        &cold_samples,
        &hot_samples,
        &cycle_summaries,
        &indexed_repos,
        "finalizing",
        Some(args.cycles.saturating_sub(1)),
        None,
        None,
    )?)?;

    let summary = json!({
        "cold_benchmark": {
            "profile": {
                "display_name": manifest.profile.display_name,
                "summary": manifest.profile.summary,
                "target_p50_ms": manifest.profile.target_p50_ms,
                "target_p95_ms": manifest.profile.target_p95_ms,
                "target_p99_ms": manifest.profile.target_p99_ms,
                "target_max_ms": manifest.profile.target_max_ms,
                "min_precision": manifest.profile.min_precision,
                "min_target_hit_rate": manifest.profile.min_target_hit_rate,
                "min_recall": manifest.profile.min_recall,
                "min_sample_count": manifest.profile.min_sample_count,
                "min_repo_count": manifest.profile.min_repo_count,
                "min_query_slice_count": manifest.profile.min_query_slice_count,
                "max_duration_seconds": manifest.profile.max_duration_seconds,
                "max_leakage": manifest.profile.max_leakage,
                "max_error_rate": manifest.profile.max_error_rate,
            },
            "executive_summary": {
                "verdict": verdict,
                "why": verdict_reason(
                    &manifest.profile,
                    &cold_distribution,
                    &cold_quality,
                    measured_repo_count,
                    measured_query_slice_count,
                    duration_seconds,
                    stop_reason.as_ref(),
                ),
                "cold_targets": {
                    "p50_le_target": cold_distribution.p50 <= manifest.profile.target_p50_ms,
                    "p95_le_target": cold_distribution.p95 <= manifest.profile.target_p95_ms,
                    "p99_le_target": cold_distribution.p99 <= manifest.profile.target_p99_ms,
                    "max_le_target": cold_distribution.max <= manifest.profile.target_max_ms,
                    "sample_count_ge_target": cold_distribution.sample_count as u64 >= manifest.profile.min_sample_count,
                    "repo_count_ge_target": measured_repo_count >= manifest.profile.min_repo_count,
                    "query_slice_count_ge_target": measured_query_slice_count >= manifest.profile.min_query_slice_count,
                    "duration_le_target": duration_seconds <= manifest.profile.max_duration_seconds,
                    "leakage_eq_target": cold_quality["leakage"].as_u64().unwrap_or_default() <= manifest.profile.max_leakage,
                    "error_rate_le_target": cold_quality["error_rate"].as_f64().unwrap_or_default() <= manifest.profile.max_error_rate,
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
                "cold_p50_ms": cold_distribution.p50,
                "cold_p95_ms": cold_distribution.p95,
                "cold_p99_ms": cold_distribution.p99,
                "cold_max_ms": cold_distribution.max,
                "target_p50_ms": manifest.profile.target_p50_ms,
                "target_p95_ms": manifest.profile.target_p95_ms,
                "target_p99_ms": manifest.profile.target_p99_ms,
                "target_max_ms": manifest.profile.target_max_ms,
                "sample_count": cold_distribution.sample_count,
                "target_sample_count": manifest.profile.min_sample_count,
                "repo_count": measured_repo_count,
                "target_repo_count": manifest.profile.min_repo_count,
                "query_slice_count": measured_query_slice_count,
                "target_query_slice_count": manifest.profile.min_query_slice_count,
                "duration_seconds": duration_seconds,
                "run_wall_clock_duration_seconds": run_wall_clock_duration_seconds,
                "target_duration_seconds": manifest.profile.max_duration_seconds,
                "leakage": cold_quality["leakage"].clone(),
                "target_leakage": manifest.profile.max_leakage,
                "error_rate": cold_quality["error_rate"].clone(),
                "target_error_rate": manifest.profile.max_error_rate,
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
                "leakage": cold_quality["leakage"].as_u64().unwrap_or_default(),
                "error_rate": cold_quality["error_rate"].as_f64().unwrap_or_default(),
                "repo_count": measured_repo_count,
                "query_slice_count": measured_query_slice_count,
                "duration": duration_seconds,
                "run_wall_clock_duration": run_wall_clock_duration_seconds,
                "target_met": target_met,
                "thermal_stop_count": thermal_stop_count,
                "cleanup_actions_count": cleanup_actions_count,
            },
            "canonical_eval": canonical_eval,
            "indexed_repos": indexed_repos.iter().map(indexed_repo_to_json).collect::<Vec<_>>(),
        },
        "retrieval_science": retrieval_science::suite_metadata("cold_path_benchmark")?,
        "degradation_policy": retrieval_science::degradation_policy_json()?,
    });

    write_outputs(&output_dir, &summary, &cold_samples, &hot_samples)?;
    let _ = postgres::insert_observability_snapshot(db, "cold_path_benchmark", &summary).await?;
    progress_guard.clear();
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
            let resolved_root = resolve_relative_path(repo_root, &repo.repo_root)
                .canonicalize()
                .with_context(|| {
                    format!(
                        "failed to canonicalize cold benchmark repo_root {}",
                        resolve_relative_path(repo_root, &repo.repo_root).display()
                    )
                })?;
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
            paths_file: None,
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

fn should_index_repo_for_case(
    skip_index: bool,
    reindex_each_cycle: bool,
    repo_code: &str,
    indexed_repo_codes: &BTreeSet<String>,
    cycle_indexed_repo_codes: &BTreeSet<String>,
) -> bool {
    if skip_index {
        return false;
    }
    if reindex_each_cycle {
        !cycle_indexed_repo_codes.contains(repo_code)
    } else {
        !indexed_repo_codes.contains(repo_code)
    }
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
        limit_documents: case.limit_documents.unwrap_or(4),
        limit_symbols: case.limit_symbols.unwrap_or(4),
        limit_chunks: case.limit_chunks.unwrap_or(4),
        limit_semantic_chunks: case.limit_semantic_chunks.unwrap_or(4),
    };
    let started = Instant::now();
    let pack =
        retrieval::execute_context_pack_capture_with_options(cfg, db, &args, false, false).await?;
    let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
    let quality = evaluate_case(&pack.payload, case);
    let mut fallback_triggered = false;
    let mut fallback_elapsed_ms = 0.0;
    if cold && !quality.target_hit {
        fallback_triggered = true;
        let fallback_mode = fallback_mode(&retrieval_mode);
        let fallback_args = ContextPackArgs {
            retrieval_mode: Some(fallback_mode),
            limit_documents: fallback_limit(case.limit_documents, 8),
            limit_symbols: fallback_limit(case.limit_symbols, 8),
            limit_chunks: fallback_limit(case.limit_chunks, 8),
            limit_semantic_chunks: fallback_limit(case.limit_semantic_chunks, 8),
            ..args.clone()
        };
        let fallback_started = Instant::now();
        let _ = retrieval::execute_context_pack_capture_with_options(
            cfg,
            db,
            &fallback_args,
            false,
            false,
        )
        .await?;
        fallback_elapsed_ms = fallback_started.elapsed().as_secs_f64() * 1000.0;
    }

    Ok(CaseExecution {
        primary: sample_from_case(repo, case, cycle, cold, elapsed_ms, &pack.stats, &quality),
        fallback_triggered,
        fallback_elapsed_ms,
    })
}

#[derive(Debug, Clone)]
struct CaseExecution {
    primary: RetrievalSample,
    fallback_triggered: bool,
    fallback_elapsed_ms: f64,
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
    let eval = cold_sample_eval_verdict(quality).expect("cold sample eval verdict");
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
        leakage_count: quality.leakage_count,
        eval_verdict_class: eval.class_key,
        eval_reason: eval.reason,
    }
}

fn cold_sample_eval_verdict(quality: &QualityScore) -> Result<eval_verdict::EvalVerdict> {
    let unexpected_present = quality.total_items > quality.matched_items;
    let signals = EvalSignals {
        expected_present: Some(quality.target_hit),
        unexpected_present,
        has_expected_target: true,
        ..EvalSignals::default()
    };
    eval_verdict::derive_eval_verdict(EvalPattern::RetrievalTarget, &signals)
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
    let leakage_count = if case.expected_projects.is_empty() {
        0
    } else {
        items
            .iter()
            .filter_map(|item| item["project_code"].as_str())
            .filter(|project| {
                !case
                    .expected_projects
                    .iter()
                    .any(|expected| expected == project)
            })
            .count() as u64
    };

    let mut matched_targets = 0usize;
    let mut total_targets = 0usize;
    let has_direct_retrieval_targets = !case.expected_paths.is_empty()
        || !case.expected_symbols.is_empty()
        || !case.expected_terms.is_empty();
    if !has_direct_retrieval_targets {
        for expected in &case.expected_projects {
            total_targets += 1;
            if items
                .iter()
                .any(|item| item["project_code"].as_str() == Some(expected.as_str()))
            {
                matched_targets += 1;
            }
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
        leakage_count,
        total_items,
        matched_items,
    }
}

fn build_cold_benchmark_canonical_eval(samples: &[RetrievalSample]) -> Result<Value> {
    let summary = eval_verdict::summarize_eval_layer(
        samples
            .iter()
            .map(|sample| sample.eval_verdict_class.as_str()),
    )?;
    Ok(json!({
        "eval_verdict_model_version": summary["eval_verdict_model_version"].clone(),
        "verdict_order": summary["verdict_order"].clone(),
        "verdict_counts": summary["verdict_counts"].clone(),
        "verdict_catalog": summary["verdict_catalog"].clone(),
        "probes": samples.iter().map(|sample| {
            json!({
                "name": format!(
                    "cold_case_{}_{}_cycle_{}",
                    sample.repo_code,
                    sample.query_slice,
                    sample.cycle,
                ),
                "mode": sample.mode,
                "repo_code": sample.repo_code,
                "query_slice": sample.query_slice,
                "query": sample.query,
                "eval_verdict_class": sample.eval_verdict_class,
                "eval_reason": sample.eval_reason,
                "precision": sample.precision,
                "recall": sample.recall,
                "target_hit": sample.target_hit,
                "head_hit": sample.head_hit,
                "leakage_count": sample.leakage_count,
            })
        }).collect::<Vec<_>>(),
    }))
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
    let has_direct_retrieval_targets = !case.expected_paths.is_empty()
        || !case.expected_symbols.is_empty()
        || !case.expected_terms.is_empty();
    if !has_direct_retrieval_targets
        && !case.expected_projects.is_empty()
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

fn fallback_limit(configured: Option<usize>, fallback_default: usize) -> usize {
    match configured {
        Some(0) => 0,
        Some(limit) => limit.max(1),
        None => fallback_default,
    }
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
    let leakage_total = samples
        .iter()
        .map(|sample| sample.leakage_count)
        .sum::<u64>();
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
        "leakage": leakage_total,
        "error_rate": 0.0,
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
    let repos_with_queries = samples
        .iter()
        .map(|sample| sample.repo_code.clone())
        .collect::<BTreeSet<_>>();
    let repo_types = repo_map
        .values()
        .map(|repo| repo.manifest.repo_type.clone())
        .collect::<BTreeSet<_>>();
    let size_classes = repo_map
        .values()
        .map(|repo| repo.manifest.size_class.clone())
        .collect::<BTreeSet<_>>();
    json!({
        "repo_count": repos_with_queries.len(),
        "repo_types": repo_types,
        "size_classes": size_classes,
        "repos_with_queries": repos_with_queries,
    })
}

fn query_coverage(samples: &[RetrievalSample]) -> Value {
    let mut per_slice = BTreeMap::<String, Vec<RetrievalSample>>::new();
    let mut workload_slices = BTreeSet::new();
    for sample in samples {
        per_slice
            .entry(sample.query_slice.clone())
            .or_default()
            .push(sample.clone());
        workload_slices.insert(format!(
            "{}::{}::{}",
            sample.repo_code, sample.query_slice, sample.query
        ));
    }
    json!({
        "query_slice_count": workload_slices.len(),
        "query_type_count": per_slice.len(),
        "query_slices": per_slice.keys().cloned().collect::<Vec<_>>(),
        "workload_slices": workload_slices.into_iter().collect::<Vec<_>>(),
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
    repo_count: u64,
    query_slice_count: u64,
    duration_seconds: f64,
    stop_reason: Option<&String>,
) -> &'static str {
    if stop_reason.is_some() {
        return "NOT MET";
    }
    let precision = cold_quality["precision"].as_f64().unwrap_or_default();
    let recall = cold_quality["recall"].as_f64().unwrap_or_default();
    let hit_rate = cold_quality["target_hit_rate"].as_f64().unwrap_or_default();
    let leakage = cold_quality["leakage"].as_u64().unwrap_or_default();
    let error_rate = cold_quality["error_rate"].as_f64().unwrap_or_default();
    let latency_met = cold_distribution.p50 <= profile.target_p50_ms
        && cold_distribution.p95 <= profile.target_p95_ms
        && cold_distribution.p99 <= profile.target_p99_ms
        && cold_distribution.max <= profile.target_max_ms;
    let quality_met = precision >= profile.min_precision
        && recall >= profile.min_recall
        && hit_rate >= profile.min_target_hit_rate;
    let coverage_met = cold_distribution.sample_count as u64 >= profile.min_sample_count
        && repo_count >= profile.min_repo_count
        && query_slice_count >= profile.min_query_slice_count;
    let safety_met = duration_seconds <= profile.max_duration_seconds
        && leakage <= profile.max_leakage
        && error_rate <= profile.max_error_rate;
    if latency_met && quality_met && coverage_met && safety_met {
        "TARGET MET"
    } else if quality_met && cold_distribution.sample_count > 0 && leakage <= profile.max_leakage {
        "PARTIALLY MET"
    } else {
        "NOT MET"
    }
}

fn verdict_reason(
    profile: &ColdBenchmarkProfile,
    cold_distribution: &Distribution,
    cold_quality: &Value,
    repo_count: u64,
    query_slice_count: u64,
    duration_seconds: f64,
    stop_reason: Option<&String>,
) -> String {
    if let Some(reason) = stop_reason {
        return format!("Run stopped by safety guard: {reason}");
    }
    let precision = cold_quality["precision"].as_f64().unwrap_or_default();
    let recall = cold_quality["recall"].as_f64().unwrap_or_default();
    let hit_rate = cold_quality["target_hit_rate"].as_f64().unwrap_or_default();
    let leakage = cold_quality["leakage"].as_u64().unwrap_or_default();
    let error_rate = cold_quality["error_rate"].as_f64().unwrap_or_default();
    let mut reasons = Vec::new();
    if cold_distribution.p50 > profile.target_p50_ms {
        reasons.push(format!(
            "cold p50 {:.3} ms выше целевого {:.3} ms",
            cold_distribution.p50, profile.target_p50_ms
        ));
    }
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
    if (cold_distribution.sample_count as u64) < profile.min_sample_count {
        reasons.push(format!(
            "sample count {} ниже допустимого {}",
            cold_distribution.sample_count, profile.min_sample_count
        ));
    }
    if repo_count < profile.min_repo_count {
        reasons.push(format!(
            "repo count {} ниже допустимого {}",
            repo_count, profile.min_repo_count
        ));
    }
    if query_slice_count < profile.min_query_slice_count {
        reasons.push(format!(
            "query slice count {} ниже допустимого {}",
            query_slice_count, profile.min_query_slice_count
        ));
    }
    if duration_seconds > profile.max_duration_seconds {
        reasons.push(format!(
            "duration {:.2} сек. выше допустимого {:.2} сек.",
            duration_seconds, profile.max_duration_seconds
        ));
    }
    if leakage > profile.max_leakage {
        reasons.push(format!(
            "leakage {} выше допустимого {}",
            leakage, profile.max_leakage
        ));
    }
    if error_rate > profile.max_error_rate {
        reasons.push(format!(
            "error rate {:.4} выше допустимого {:.4}",
            error_rate, profile.max_error_rate
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

fn cold_profile_to_json(profile: &ColdBenchmarkProfile) -> Value {
    json!({
        "display_name": profile.display_name,
        "summary": profile.summary,
        "target_p50_ms": profile.target_p50_ms,
        "target_p95_ms": profile.target_p95_ms,
        "target_p99_ms": profile.target_p99_ms,
        "target_max_ms": profile.target_max_ms,
        "min_precision": profile.min_precision,
        "min_target_hit_rate": profile.min_target_hit_rate,
        "min_recall": profile.min_recall,
        "min_sample_count": profile.min_sample_count,
        "min_repo_count": profile.min_repo_count,
        "min_query_slice_count": profile.min_query_slice_count,
        "max_duration_seconds": profile.max_duration_seconds,
        "max_leakage": profile.max_leakage,
        "max_error_rate": profile.max_error_rate,
    })
}

fn build_live_progress_payload(
    output_dir: &Path,
    manifest: &ColdBenchmarkManifest,
    args: &VerifyColdPathArgs,
    repo_map: &BTreeMap<String, RepoRuntime>,
    repo_target_file_counts: &BTreeMap<String, usize>,
    run_started_epoch_ms: u64,
    cold_samples: &[RetrievalSample],
    hot_samples: &[RetrievalSample],
    cycle_summaries: &[CycleSummary],
    indexed_repos: &[IndexedRepoSummary],
    phase: &str,
    current_cycle: Option<usize>,
    current_repo_code: Option<&str>,
    current_query_slice: Option<&str>,
) -> Result<Value> {
    let cold_distribution = distribution_from_samples(cold_samples);
    let hot_distribution = distribution_from_samples(hot_samples);
    let cold_quality = summarize_quality(cold_samples);
    let hot_quality = summarize_quality(hot_samples);
    let repo_coverage = repo_coverage(repo_map, cold_samples);
    let query_coverage = query_coverage(cold_samples);
    let measured_repo_count = repo_coverage["repo_count"].as_u64().unwrap_or_default();
    let measured_query_slice_count = query_coverage["query_slice_count"]
        .as_u64()
        .unwrap_or_default();
    let cold_workload_duration_seconds = cold_samples
        .iter()
        .map(|sample| sample.total_ms)
        .sum::<f64>()
        / 1000.0;
    let run_wall_clock_duration_seconds =
        (now_epoch_ms().saturating_sub(run_started_epoch_ms)) as f64 / 1000.0;
    let completed_case_count = cold_samples.len() as u64;
    let target_case_count = (manifest.cases.len() * args.cycles) as u64;
    let progress_ratio = if target_case_count == 0 {
        0.0
    } else {
        completed_case_count as f64 / target_case_count as f64
    };
    let current_repo_target_files = current_repo_code
        .and_then(|code| repo_target_file_counts.get(code))
        .copied()
        .map(|value| value as u64);
    let current_repo_display_name = current_repo_code
        .and_then(|code| repo_map.get(code))
        .map(|repo| repo.manifest.display_name.clone());

    Ok(json!({
        "cold_benchmark_progress": {
            "state": "running",
            "pid": process::id(),
            "captured_at_epoch_ms": now_epoch_ms(),
            "started_at_epoch_ms": run_started_epoch_ms,
            "phase": phase,
            "current_cycle": current_cycle.map(|value| value as u64 + 1),
            "total_cycles": args.cycles,
            "current_repo_code": current_repo_code,
            "current_repo_display_name": current_repo_display_name,
            "current_query_slice": current_query_slice,
            "output_dir": output_dir.display().to_string(),
            "profile": cold_profile_to_json(&manifest.profile),
            "executive_summary": {
                "verdict": "RUNNING",
                "why": format!(
                    "Идёт живой cold benchmark: завершено {} из {} cold-case, фаза {}.",
                    completed_case_count,
                    target_case_count,
                    phase
                ),
            },
            "progress": {
                "completed_case_count": completed_case_count,
                "target_case_count": target_case_count,
                "completed_ratio": progress_ratio,
                "repo_count": measured_repo_count,
                "query_slice_count": measured_query_slice_count,
                "current_cycle": current_cycle.map(|value| value as u64 + 1),
                "total_cycles": args.cycles,
                "current_repo_target_files": current_repo_target_files,
            },
            "cold_latency_distribution": distribution_to_json(&cold_distribution),
            "hot_latency_distribution": distribution_to_json(&hot_distribution),
            "quality_metrics": {
                "cold": cold_quality.clone(),
                "hot": hot_quality,
            },
            "long_run_stability": {
                "cycles": cycle_summaries.iter().map(cycle_summary_to_json).collect::<Vec<_>>(),
                "summary": long_run_summary(cycle_summaries),
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
                "leakage": cold_quality["leakage"].as_u64().unwrap_or_default(),
                "error_rate": cold_quality["error_rate"].as_f64().unwrap_or_default(),
                "repo_count": measured_repo_count,
                "query_slice_count": measured_query_slice_count,
                "duration": cold_workload_duration_seconds,
                "run_wall_clock_duration": run_wall_clock_duration_seconds,
                "target_met": false,
                "thermal_stop_count": 0,
                "cleanup_actions_count": 0,
            },
            "dataset_coverage": repo_coverage,
            "query_coverage": query_coverage,
            "indexed_repos": indexed_repos.iter().map(indexed_repo_to_json).collect::<Vec<_>>(),
        },
        "retrieval_science": retrieval_science::suite_metadata("cold_path_benchmark")?,
        "degradation_policy": retrieval_science::degradation_policy_json()?,
    }))
}

fn live_progress_cache_path(repo_root: &Path) -> PathBuf {
    repo_root
        .join("state")
        .join("cold-benchmark")
        .join("live_progress.json")
}

fn output_progress_path(output_dir: &Path) -> PathBuf {
    output_dir.join("progress.json")
}

fn now_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
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
        "mode,cycle,repo_code,repo_type,size_class,query_slice,query,total_ms,policy_ms,retrieval_ms,ranking_ms,provenance_ms,pack_assembly_ms,orchestration_ms,precision,recall,target_hit,miss,fallback_triggered,head_hit,eval_verdict_class,eval_reason\n",
    );
    for sample in cold_samples.iter().chain(hot_samples.iter()) {
        output.push_str(&format!(
            "{},{},{},{},{},{},{:?},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.4},{:.4},{},{},{},{},{},{:?}\n",
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
            sample.eval_verdict_class,
            sample.eval_reason,
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
    use super::{
        ColdBenchmarkCase, ColdBenchmarkManifest, ColdBenchmarkProfile, ColdBenchmarkRepo,
        CycleSummary, IndexedRepoSummary, QualityScore, RepoRuntime,
        build_cold_benchmark_canonical_eval, build_live_progress_payload, cold_sample_eval_verdict,
        determine_verdict, distribution_from_f64, evaluate_case, should_index_repo_for_case,
    };
    use crate::cli::VerifyColdPathArgs;
    use crate::cold_benchmark::item_matches_case;
    use serde_json::json;
    use std::collections::{BTreeMap, BTreeSet};
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn evaluate_case_uses_expected_paths_terms_and_symbols() {
        let payload = json!({
            "retrieval": {
                "exact_documents": [
                    {
                        "project_code": "amai",
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
            repo_code: "amai".to_string(),
            query_slice: "symbol_lookup".to_string(),
            query: "run_text_compare".to_string(),
            retrieval_mode: None,
            limit_documents: None,
            limit_symbols: None,
            limit_chunks: None,
            limit_semantic_chunks: None,
            expected_projects: vec!["amai".to_string()],
            expected_paths: vec!["src/verify.rs".to_string()],
            expected_terms: vec!["run_text_compare".to_string()],
            expected_symbols: vec!["run_text_compare".to_string()],
        };
        let score = evaluate_case(&payload, &case);
        assert!(score.target_hit);
        assert_eq!(score.precision, 1.0);
        assert_eq!(score.recall, 1.0);
        assert!(score.head_hit);
        assert_eq!(score.total_items, 1);
        assert_eq!(score.matched_items, 1);
    }

    #[test]
    fn evaluate_case_does_not_penalize_exact_path_for_project_target() {
        let payload = json!({
            "retrieval": {
                "exact_documents": [
                    {
                        "relative_path": "README.md",
                        "snippet": "example readme"
                    }
                ],
                "symbol_hits": [],
                "lexical_chunks": [],
                "semantic_chunks": []
            }
        });
        let case = ColdBenchmarkCase {
            repo_code: "ripgrep".to_string(),
            query_slice: "docs_lookup".to_string(),
            query: "README.md".to_string(),
            retrieval_mode: None,
            limit_documents: None,
            limit_symbols: None,
            limit_chunks: None,
            limit_semantic_chunks: None,
            expected_projects: vec!["ripgrep".to_string()],
            expected_paths: vec!["README.md".to_string()],
            expected_terms: Vec::new(),
            expected_symbols: Vec::new(),
        };
        let score = evaluate_case(&payload, &case);
        assert_eq!(score.precision, 1.0);
        assert_eq!(score.recall, 1.0);
        assert!(score.target_hit);
    }

    #[test]
    fn cold_sample_eval_marks_wrong_target_when_only_wrong_items_arrive() {
        let verdict = cold_sample_eval_verdict(&QualityScore {
            precision: 0.0,
            recall: 0.0,
            target_hit: false,
            head_hit: false,
            leakage_count: 1,
            total_items: 2,
            matched_items: 0,
        })
        .expect("verdict");
        assert_eq!(verdict.class_key, "hit_wrong_target");
    }

    #[test]
    fn resolve_repos_canonicalizes_relative_repo_root_segments() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("amai-cold-manifest-root-{unique}"));
        let repo = root.join("repo");
        fs::create_dir_all(&repo).expect("create repo");

        let runtimes = super::resolve_repos(
            &root,
            &[ColdBenchmarkRepo {
                code: "demo".to_string(),
                display_name: "Demo".to_string(),
                repo_root: PathBuf::from("repo/../repo"),
                namespace: "cold_benchmark".to_string(),
                repo_type: "mixed".to_string(),
                size_class: "small".to_string(),
                limit_files: None,
                skip_embeddings: true,
                default_retrieval_mode: "local_strict".to_string(),
            }],
        )
        .expect("resolve repos");

        assert_eq!(runtimes[0].resolved_root, repo);

        fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn repo_indexing_is_lazy_per_repo_when_cycle_reindex_is_disabled() {
        let mut indexed_repo_codes = BTreeSet::new();
        let cycle_indexed_repo_codes = BTreeSet::new();
        assert!(should_index_repo_for_case(
            false,
            false,
            "art",
            &indexed_repo_codes,
            &cycle_indexed_repo_codes,
        ));
        indexed_repo_codes.insert("art".to_string());
        assert!(!should_index_repo_for_case(
            false,
            false,
            "art",
            &indexed_repo_codes,
            &cycle_indexed_repo_codes,
        ));
        assert!(should_index_repo_for_case(
            false,
            false,
            "amai",
            &indexed_repo_codes,
            &cycle_indexed_repo_codes,
        ));
    }

    #[test]
    fn repo_indexing_repeats_each_cycle_only_after_first_use_in_that_cycle() {
        let mut indexed_repo_codes = BTreeSet::new();
        indexed_repo_codes.insert("art".to_string());
        let mut cycle_indexed_repo_codes = BTreeSet::new();
        assert!(should_index_repo_for_case(
            false,
            true,
            "art",
            &indexed_repo_codes,
            &cycle_indexed_repo_codes,
        ));
        cycle_indexed_repo_codes.insert("art".to_string());
        assert!(!should_index_repo_for_case(
            false,
            true,
            "art",
            &indexed_repo_codes,
            &cycle_indexed_repo_codes,
        ));
    }

    #[test]
    fn cold_benchmark_canonical_eval_summarizes_probe_counts() {
        let summary = build_cold_benchmark_canonical_eval(&[
            super::RetrievalSample {
                cycle: 0,
                mode: "cold",
                repo_code: "repo_a".to_string(),
                repo_type: "mixed".to_string(),
                size_class: "small".to_string(),
                query_slice: "docs_lookup".to_string(),
                query: "README.md".to_string(),
                total_ms: 1.0,
                policy_ms: 0.1,
                retrieval_ms: 0.2,
                ranking_ms: 0.1,
                provenance_ms: 0.1,
                pack_assembly_ms: 0.1,
                orchestration_ms: 1.0,
                precision: 1.0,
                recall: 1.0,
                target_hit: true,
                miss: false,
                fallback_triggered: false,
                head_hit: true,
                leakage_count: 0,
                eval_verdict_class: "hit_correct_target".to_string(),
                eval_reason: "ok".to_string(),
            },
            super::RetrievalSample {
                cycle: 0,
                mode: "cold",
                repo_code: "repo_b".to_string(),
                repo_type: "mixed".to_string(),
                size_class: "small".to_string(),
                query_slice: "docs_lookup".to_string(),
                query: "missing".to_string(),
                total_ms: 1.0,
                policy_ms: 0.1,
                retrieval_ms: 0.2,
                ranking_ms: 0.1,
                provenance_ms: 0.1,
                pack_assembly_ms: 0.1,
                orchestration_ms: 1.0,
                precision: 0.0,
                recall: 0.0,
                target_hit: false,
                miss: true,
                fallback_triggered: false,
                head_hit: false,
                leakage_count: 0,
                eval_verdict_class: "under_retrieved".to_string(),
                eval_reason: "miss".to_string(),
            },
        ])
        .expect("summary");
        assert_eq!(
            summary["verdict_counts"]["hit_correct_target"].as_u64(),
            Some(1)
        );
        assert_eq!(
            summary["verdict_counts"]["under_retrieved"].as_u64(),
            Some(1)
        );
        assert_eq!(summary["probes"].as_array().map(Vec::len), Some(2));
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
            target_p50_ms: 10.0,
            target_p95_ms: 10.0,
            target_p99_ms: 15.0,
            target_max_ms: 20.0,
            min_precision: 0.99,
            min_target_hit_rate: 0.99,
            min_recall: 0.99,
            min_sample_count: 1,
            min_repo_count: 1,
            min_query_slice_count: 1,
            max_duration_seconds: 30.0,
            max_leakage: 0,
            max_error_rate: 0.0,
        };
        let quality = json!({
            "precision": 1.0,
            "recall": 1.0,
            "target_hit_rate": 1.0,
            "leakage": 0,
            "error_rate": 0.0,
        });
        let distribution = distribution_from_f64(&[5.0, 12.0, 16.0, 20.0]);
        assert_eq!(
            determine_verdict(&profile, &distribution, &quality, 1, 1, 1.0, None),
            "PARTIALLY MET"
        );
    }

    #[test]
    fn item_matches_case_checks_multiple_evidence_types() {
        let item = json!({
            "project_code": "amai",
            "relative_path": "README.md",
            "name": "install_amai",
            "snippet": "install_amai.sh"
        });
        let case = ColdBenchmarkCase {
            repo_code: "amai".to_string(),
            query_slice: "onboarding_query".to_string(),
            query: "install_amai.sh".to_string(),
            retrieval_mode: None,
            limit_documents: None,
            limit_symbols: None,
            limit_chunks: None,
            limit_semantic_chunks: None,
            expected_projects: vec!["amai".to_string()],
            expected_paths: vec!["README.md".to_string()],
            expected_terms: vec!["install_amai.sh".to_string()],
            expected_symbols: vec!["install_amai".to_string()],
        };
        assert!(item_matches_case(&item, &case));
    }

    #[test]
    fn live_progress_duration_uses_only_completed_cold_cases() {
        let manifest = ColdBenchmarkManifest {
            profile: ColdBenchmarkProfile {
                display_name: "proof".to_string(),
                summary: "proof".to_string(),
                target_p50_ms: 2.0,
                target_p95_ms: 5.0,
                target_p99_ms: 10.0,
                target_max_ms: 15.0,
                min_precision: 0.997,
                min_target_hit_rate: 0.997,
                min_recall: 0.997,
                min_sample_count: 1000,
                min_repo_count: 75,
                min_query_slice_count: 200,
                max_duration_seconds: 10.0,
                max_leakage: 0,
                max_error_rate: 0.0,
            },
            repos: vec![ColdBenchmarkRepo {
                code: "art".to_string(),
                display_name: "Art".to_string(),
                repo_root: PathBuf::from("/tmp/art"),
                namespace: "cold_benchmark".to_string(),
                repo_type: "mixed".to_string(),
                size_class: "large_monorepo".to_string(),
                limit_files: Some(10),
                skip_embeddings: true,
                default_retrieval_mode: "local_strict".to_string(),
            }],
            cases: vec![ColdBenchmarkCase {
                repo_code: "art".to_string(),
                query_slice: "docs_lookup".to_string(),
                query: "README.md".to_string(),
                retrieval_mode: None,
                limit_documents: Some(1),
                limit_symbols: Some(0),
                limit_chunks: Some(0),
                limit_semantic_chunks: Some(0),
                expected_projects: vec!["art".to_string()],
                expected_paths: vec!["README.md".to_string()],
                expected_terms: Vec::new(),
                expected_symbols: Vec::new(),
            }],
        };
        let args = VerifyColdPathArgs {
            manifest: PathBuf::from("config/cold_benchmark_manifest.toml"),
            cycles: 1,
            thermal_guard_celsius: 85.0,
            cooldown_seconds: 20,
            max_cooldown_retries: 2,
            min_disk_free_gib: 25.0,
            reindex_each_cycle: false,
            skip_index: false,
            output_dir: PathBuf::from("state/cold-benchmark/latest"),
        };
        let mut repo_map = BTreeMap::new();
        repo_map.insert(
            "art".to_string(),
            RepoRuntime {
                manifest: manifest.repos[0].clone(),
                resolved_root: PathBuf::from("/tmp/art"),
            },
        );
        let mut repo_target_file_counts = BTreeMap::new();
        repo_target_file_counts.insert("art".to_string(), 10usize);
        let payload = build_live_progress_payload(
            PathBuf::from("state/cold-benchmark/latest").as_path(),
            &manifest,
            &args,
            &repo_map,
            &repo_target_file_counts,
            super::now_epoch_ms().saturating_sub(30_000),
            &[],
            &[],
            &Vec::<CycleSummary>::new(),
            &Vec::<IndexedRepoSummary>::new(),
            "indexing",
            Some(0),
            Some("art"),
            None,
        )
        .expect("payload");
        assert_eq!(
            payload["cold_benchmark_progress"]["machine_readable_summary"]["duration"].as_f64(),
            Some(0.0)
        );
        assert!(
            payload["cold_benchmark_progress"]["machine_readable_summary"]["run_wall_clock_duration"]
                .as_f64()
                .unwrap_or_default()
                > 0.0
        );
    }
}
