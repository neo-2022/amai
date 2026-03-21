use crate::external_benchmark_conversion::{VectorDbBenchBundle, ensure_vectordbbench_bundle};
use anyhow::{Context, Result, anyhow};
use reqwest::Client;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::fs as tokio_fs;
use tokio::io::AsyncWriteExt;

const AMAI_VDBBENCH_QDRANT_TIMEOUT_SECONDS: u32 = 600;
const AMAI_VDBBENCH_QDRANT_CLIENT_VERSION: &str = "1.12.2";
const AMAI_VDBBENCH_QDRANT_HTTP_URL_FALLBACK: &str = "http://127.0.0.1:6333";
const AMAI_VDBBENCH_QDRANT_IMAGE: &str = "qdrant/qdrant:v1.12.5";

#[derive(Debug, Deserialize)]
struct ExternalBenchmarkFile {
    source: ExternalBenchmarkSource,
    benchmarks: BTreeMap<String, ExternalBenchmarkEntry>,
}

#[derive(Debug, Deserialize)]
struct ExternalDatasetFile {
    source: ExternalBenchmarkSource,
    storage: ExternalDatasetStorage,
    datasets: BTreeMap<String, ExternalDatasetEntry>,
}

#[derive(Debug, Clone, Deserialize)]
struct ExternalBenchmarkSource {
    display_name: String,
    summary: String,
}

#[derive(Debug, Clone, Deserialize)]
struct ExternalDatasetStorage {
    relative_root: String,
}

#[derive(Debug, Clone, Deserialize)]
struct ExternalBenchmarkEntry {
    order: u32,
    display_name: String,
    benchmark_kind: String,
    summary: String,
    reference_url: String,
    upstream_git_url: String,
    #[serde(default)]
    aliases: Vec<String>,
    requires_tools: Vec<String>,
    why_relevant: Vec<String>,
    local_role: Vec<String>,
    next_step: String,
}

#[derive(Debug, Clone, Deserialize)]
struct ExternalDatasetEntry {
    order: u32,
    display_name: String,
    #[serde(default)]
    aliases: Vec<String>,
    family: String,
    distance: String,
    dimensions: u32,
    local_filename: String,
    download_url: String,
    usage_scope: Vec<String>,
    why_useful: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AdapterStatus {
    Prepared,
    BlockedUnsupportedDataset,
    BlockedUpstreamDisabled,
    BlockedConversionRequired,
    BlockedDatasetMissing,
}

struct AdapterRenderContext<'a> {
    benchmark_code: &'a str,
    benchmark: &'a ExternalBenchmarkEntry,
    dataset_code: &'a str,
    dataset: &'a ExternalDatasetEntry,
    dataset_path: &'a Path,
    status: AdapterStatus,
    adapter_kind: &'a str,
    launch_commands: &'a [String],
    comparison_commands: &'a [String],
    compatibility_overrides: &'a [String],
    upstream_dir: &'a Path,
}

#[derive(Debug, Clone)]
struct ToolCheck {
    available: bool,
    version: String,
}

#[derive(Debug, Clone)]
struct UpstreamCheck {
    reachable: bool,
    head: String,
}

#[derive(Debug)]
struct ExternalResultSummary {
    path: PathBuf,
    run_id: String,
    task_label: String,
    db: String,
    label: String,
    qps: Option<f64>,
    serial_latency_p99: Option<f64>,
    serial_latency_p95: Option<f64>,
    recall: Option<f64>,
    max_load_count: Option<f64>,
    load_duration: Option<f64>,
}

pub fn print_external_check(repo_root: &Path) -> Result<()> {
    let registry = load_registry(repo_root)?;
    let required_tools = registry
        .benchmarks
        .values()
        .flat_map(|entry| entry.requires_tools.iter().cloned())
        .collect::<BTreeSet<_>>();
    let tool_checks = required_tools
        .iter()
        .map(|tool| (tool.clone(), inspect_tool(tool)))
        .collect::<BTreeMap<_, _>>();
    let docker_daemon_ok = if required_tools.contains("docker") {
        command_ok("docker", &["info"])
    } else {
        true
    };
    let upstream_checks = registry
        .benchmarks
        .iter()
        .map(|(code, entry)| (code.clone(), inspect_upstream(&entry.upstream_git_url)))
        .collect::<BTreeMap<_, _>>();

    println!("Amai external benchmark readiness");
    println!();
    println!("Источник: {}", registry.source.display_name);
    println!("{}", registry.source.summary);
    println!();
    println!(
        "Эта проверка отвечает на вопрос: можно ли на этой машине честно запускать внешний comparative benchmark contour, не подменяя им внутренний cold/hot путь Amai."
    );
    println!();
    println!("Локальная среда:");
    for (tool, check) in &tool_checks {
        if check.available {
            println!("- {}: {}", tool, check.version);
        } else {
            println!("- {}: отсутствует", tool);
        }
    }
    if required_tools.contains("docker") {
        println!(
            "- docker daemon: {}",
            if docker_daemon_ok {
                "доступен"
            } else {
                "недоступен"
            }
        );
    }
    println!();

    let mut ready = 0usize;
    let mut blocked = 0usize;
    for (code, entry) in ordered_benchmarks(&registry) {
        let upstream = upstream_checks
            .get(code)
            .ok_or_else(|| anyhow!("missing upstream check for {}", code))?;
        let tools_ready = entry.requires_tools.iter().all(|tool| {
            tool_checks
                .get(tool)
                .map(|check| check.available)
                .unwrap_or(false)
        });
        let runtime_ready = tools_ready
            && upstream.reachable
            && (!entry.requires_tools.iter().any(|tool| tool == "docker") || docker_daemon_ok);
        if runtime_ready {
            ready += 1;
        } else {
            blocked += 1;
        }

        println!("{} ({})", entry.display_name, code);
        println!("- Тип: {}", entry.benchmark_kind);
        println!(
            "- Статус готовности: {}",
            if runtime_ready {
                "готов к локальному прогону"
            } else {
                "пока заблокирован"
            }
        );
        println!(
            "- Upstream: {}",
            if upstream.reachable {
                format!("доступен ({})", upstream.head)
            } else {
                "недоступен".to_owned()
            }
        );
        println!("- Ссылка: {}", entry.reference_url);
        println!("- Что даёт для Amai: {}", entry.summary);
        println!("- Почему нужен:");
        for item in &entry.why_relevant {
            println!("  - {}", item);
        }
        println!("- Роль в локальном comparative contour:");
        for item in &entry.local_role {
            println!("  - {}", item);
        }
        println!("- Следующий шаг: {}", entry.next_step);
        println!();
    }

    println!("Итог:");
    println!("- Готово к локальному прогону: {}", ready);
    println!("- Заблокировано: {}", blocked);
    println!();
    println!("Рекомендуемый порядок для Amai:");
    println!("- 1. General framework + adapter: VectorDBBench");
    println!("- 2. Ceiling retrieval-core: ann-benchmarks");
    println!("- 3. Filter/payload pressure: filtered ANN datasets");
    println!("- 4. Затем сопоставить результаты с внутренним Amai end-to-end cold/hot contour.");
    Ok(())
}

pub fn print_external_explainer(repo_root: &Path, benchmark_query: &str) -> Result<()> {
    let registry = load_registry(repo_root)?;
    let (code, entry) = resolve_benchmark(&registry, benchmark_query)
        .ok_or_else(|| anyhow!("unknown external benchmark: {benchmark_query}"))?;

    println!("Amai external benchmark explainer");
    println!();
    println!("Benchmark: {} ({})", entry.display_name, code);
    println!("Тип: {}", entry.benchmark_kind);
    println!("Ссылка: {}", entry.reference_url);
    println!();
    println!("Что это за benchmark:");
    println!("{}", entry.summary);
    println!();
    println!("Почему он важен для Amai:");
    for item in &entry.why_relevant {
        println!("- {}", item);
    }
    println!();
    println!("Как его правильно использовать у нас:");
    for item in &entry.local_role {
        println!("- {}", item);
    }
    println!();
    println!("Следующий шаг:");
    println!("- {}", entry.next_step);
    Ok(())
}

pub fn print_external_datasets(repo_root: &Path) -> Result<()> {
    let catalog = load_dataset_catalog(repo_root)?;
    let dataset_root = dataset_root(repo_root, &catalog.storage.relative_root);

    println!("Amai external benchmark datasets");
    println!();
    println!("Источник: {}", catalog.source.display_name);
    println!("{}", catalog.source.summary);
    println!();
    println!("Канонический локальный каталог:");
    println!("- {}", dataset_root.display());
    println!();

    for (code, dataset) in ordered_datasets(&catalog) {
        let path = dataset_root.join(&dataset.local_filename);
        let (status, size) = if path.exists() {
            let metadata = fs::metadata(&path)
                .with_context(|| format!("failed to stat dataset {}", path.display()))?;
            ("уже скачан", format_bytes(metadata.len()))
        } else {
            ("ещё не скачан", "0 B".to_owned())
        };
        println!("{} ({})", dataset.display_name, code);
        println!("- Семейство: {}", dataset.family);
        println!("- Distance: {}", dataset.distance);
        println!("- Размерность: {}", dataset.dimensions);
        println!("- Локальный файл: {}", path.display());
        println!("- Статус: {} ({})", status, size);
        println!("- Скачать: {}", dataset.download_url);
        if !dataset.aliases.is_empty() {
            println!("- Псевдонимы: {}", dataset.aliases.join(", "));
        }
        println!("- Где применять:");
        for item in &dataset.usage_scope {
            println!("  - {}", item);
        }
        println!("- Почему полезен:");
        for item in &dataset.why_useful {
            println!("  - {}", item);
        }
        println!();
    }
    Ok(())
}

pub fn print_external_plan(repo_root: &Path, benchmark_query: &str) -> Result<()> {
    let registry = load_registry(repo_root)?;
    let catalog = load_dataset_catalog(repo_root)?;
    let (code, entry) = resolve_benchmark(&registry, benchmark_query)
        .ok_or_else(|| anyhow!("unknown external benchmark: {benchmark_query}"))?;
    let dataset_root = dataset_root(repo_root, &catalog.storage.relative_root);
    let datasets = recommended_datasets(code, &catalog);

    println!("Amai external benchmark adapter plan");
    println!();
    println!("Benchmark: {} ({})", entry.display_name, code);
    println!("Тип: {}", entry.benchmark_kind);
    println!("Ссылка: {}", entry.reference_url);
    println!();
    println!("Канонический dataset root:");
    println!("- {}", dataset_root.display());
    println!();
    println!("Рекомендуемые датасеты для этого benchmark-а:");
    for (dataset_code, dataset) in &datasets {
        println!(
            "- {} ({}) :: {} :: dim={} :: {}",
            dataset.display_name,
            dataset_code,
            dataset.distance,
            dataset.dimensions,
            dataset.download_url
        );
    }
    println!();
    println!("Amai adapter contract:");
    println!("- 1. ingest(dataset)");
    println!("- 2. warmup(dataset)");
    println!("- 3. run fixed workload");
    println!("- 4. collect latency: P50/P95/P99/Max/sample_count");
    println!("- 5. collect quality: recall/precision/hit/miss/fallback");
    println!("- 6. compare with internal Amai cold/hot contour");
    println!();
    println!("Практический порядок:");
    println!(
        "- Сначала скачать HDF5-датасеты в {}",
        dataset_root.display()
    );
    println!("- Затем прогнать внешний framework через adapter к retrieval/vector слою Amai.");
    println!("- После этого рядом прогнать внутренний end-to-end cold/hot benchmark.");
    println!();
    if code == "ann_benchmarks" {
        println!("Полезная стартовая связка для HDF5-style контура:");
        println!(
            "- cargo run -- benchmark external-adapter --benchmark ann_benchmarks --dataset dbpedia_openai_1000k_angular"
        );
        println!(
            "- bash {}/state/external-benchmarks/runs/ann_benchmarks/dbpedia_openai_1000k_angular/latest/run_external.sh",
            repo_root.display()
        );
        println!();
    }
    println!("Следующий шаг:");
    println!("- {}", entry.next_step);
    Ok(())
}

pub async fn download_datasets(
    repo_root: &Path,
    dataset_query: Option<&str>,
    force: bool,
) -> Result<()> {
    let catalog = load_dataset_catalog(repo_root)?;
    let dataset_root = dataset_root(repo_root, &catalog.storage.relative_root);
    tokio_fs::create_dir_all(&dataset_root)
        .await
        .with_context(|| format!("failed to create {}", dataset_root.display()))?;
    let selections = match dataset_query {
        Some(query) => vec![
            resolve_dataset(&catalog, query)
                .ok_or_else(|| anyhow!("unknown external dataset: {query}"))?,
        ],
        None => ordered_datasets(&catalog)
            .into_iter()
            .map(|(code, dataset)| (code.as_str(), dataset))
            .collect(),
    };

    println!("Amai external benchmark download");
    println!();
    println!("Каталог: {}", dataset_root.display());
    println!();
    for (code, dataset) in selections {
        let path = dataset_root.join(&dataset.local_filename);
        if path.exists() && !force {
            let metadata = fs::metadata(&path)
                .with_context(|| format!("failed to stat dataset {}", path.display()))?;
            println!(
                "- {} ({}) уже скачан: {}",
                dataset.display_name,
                code,
                format_bytes(metadata.len())
            );
            continue;
        }
        download_dataset_file(dataset, &path).await?;
        let metadata = fs::metadata(&path)
            .with_context(|| format!("failed to stat dataset {}", path.display()))?;
        println!(
            "- {} ({}) скачан: {}",
            dataset.display_name,
            code,
            format_bytes(metadata.len())
        );
    }
    Ok(())
}

pub async fn run_external_adapter(
    repo_root: &Path,
    benchmark_query: &str,
    dataset_query: &str,
    download_missing: bool,
    output_dir_override: Option<&Path>,
) -> Result<()> {
    let registry = load_registry(repo_root)?;
    let catalog = load_dataset_catalog(repo_root)?;
    let (benchmark_code, benchmark) = resolve_benchmark(&registry, benchmark_query)
        .ok_or_else(|| anyhow!("unknown external benchmark: {benchmark_query}"))?;
    let (dataset_code, dataset) = resolve_dataset(&catalog, dataset_query)
        .ok_or_else(|| anyhow!("unknown external dataset: {dataset_query}"))?;

    let dataset_root = dataset_root(repo_root, &catalog.storage.relative_root);
    tokio_fs::create_dir_all(&dataset_root)
        .await
        .with_context(|| format!("failed to create {}", dataset_root.display()))?;
    let dataset_path = dataset_root.join(&dataset.local_filename);
    if !dataset_path.exists() && download_missing {
        download_dataset_file(dataset, &dataset_path).await?;
    }

    let adapter_kind = adapter_kind_for(benchmark_code);
    let benchmark_qdrant_http_url = std::env::var("AMI_BENCHMARK_QDRANT_HTTP_URL")
        .unwrap_or_else(|_| AMAI_VDBBENCH_QDRANT_HTTP_URL_FALLBACK.to_owned());
    let output_dir = output_dir_override
        .map(|path| path.to_path_buf())
        .unwrap_or_else(|| {
            repo_root
                .join("state")
                .join("external-benchmarks")
                .join("runs")
                .join(benchmark_code)
                .join(dataset_code)
                .join("latest")
        });
    fs::create_dir_all(&output_dir)
        .with_context(|| format!("failed to create {}", output_dir.display()))?;
    let upstream_dir = repo_root
        .join("state")
        .join("external-benchmarks")
        .join("upstream")
        .join(benchmark_code);
    let converted_bundle = if benchmark_code == "vectordbbench"
        && benchmark_supports_dataset(benchmark_code, dataset)
        && dataset_path.exists()
    {
        Some(ensure_vectordbbench_bundle(
            repo_root,
            dataset_code,
            &dataset.display_name,
            &dataset_path,
            &dataset.distance,
            dataset.dimensions as usize,
        )?)
    } else {
        None
    };
    let status = determine_adapter_status(
        benchmark_code,
        dataset,
        dataset_path.exists(),
        &upstream_dir,
        converted_bundle.is_some(),
    );
    let launch_commands = build_launch_commands(
        benchmark_code,
        &upstream_dir,
        &dataset_path,
        dataset,
        &output_dir,
        converted_bundle.as_ref(),
        &benchmark_qdrant_http_url,
    );
    let comparison_commands = vec![
        "cargo run -- verify cold-path --manifest config/cold_benchmark_manifest.toml".to_owned(),
        "./scripts/proof_load.sh".to_owned(),
        "./scripts/proof_accuracy.sh".to_owned(),
    ];
    let compatibility_overrides = adapter_compatibility_overrides(benchmark_code);
    let summary = json!({
        "status": adapter_status_code(status),
        "benchmark_code": benchmark_code,
        "benchmark_display_name": benchmark.display_name,
        "dataset_code": dataset_code,
        "dataset_display_name": dataset.display_name,
        "dataset_path": dataset_path,
        "dataset_exists": dataset_path.exists(),
        "adapter_kind": adapter_kind,
        "output_dir": output_dir,
        "benchmark_qdrant_http_url": benchmark_qdrant_http_url,
        "upstream_repo_url": benchmark.upstream_git_url,
        "upstream_clone_dir": upstream_dir,
        "launch_commands": launch_commands,
        "comparison_commands": comparison_commands,
        "compatibility_overrides": compatibility_overrides,
        "conversion_bundle": converted_bundle,
    });
    let render_ctx = AdapterRenderContext {
        benchmark_code,
        benchmark,
        dataset_code,
        dataset,
        dataset_path: &dataset_path,
        status,
        adapter_kind,
        launch_commands: &launch_commands,
        comparison_commands: &comparison_commands,
        compatibility_overrides: &compatibility_overrides,
        upstream_dir: &upstream_dir,
    };
    let report = render_adapter_report(&render_ctx);
    let script = render_adapter_script(&render_ctx);

    let summary_path = output_dir.join("summary.json");
    let report_path = output_dir.join("report.md");
    let script_path = output_dir.join("run_external.sh");
    fs::write(&summary_path, serde_json::to_string_pretty(&summary)?)
        .with_context(|| format!("failed to write {}", summary_path.display()))?;
    fs::write(&report_path, report)
        .with_context(|| format!("failed to write {}", report_path.display()))?;
    fs::write(&script_path, script)
        .with_context(|| format!("failed to write {}", script_path.display()))?;
    let mut permissions = fs::metadata(&script_path)?.permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        permissions.set_mode(0o755);
        fs::set_permissions(&script_path, permissions)
            .with_context(|| format!("failed to chmod {}", script_path.display()))?;
    }

    println!("Amai external benchmark adapter workspace");
    println!();
    println!("Benchmark: {} ({})", benchmark.display_name, benchmark_code);
    println!("Dataset: {} ({})", dataset.display_name, dataset_code);
    println!("Статус: {}", adapter_status_label(status));
    println!("Adapter kind: {}", adapter_kind);
    println!("Output dir: {}", output_dir.display());
    println!("Summary: {}", summary_path.display());
    println!("Report: {}", report_path.display());
    println!("Run script: {}", script_path.display());
    if let Some(bundle_dir) = summary["conversion_bundle"]["bundle_dir"].as_str() {
        println!("Conversion bundle: {}", bundle_dir);
    }
    if status == AdapterStatus::BlockedDatasetMissing {
        println!("Причина: dataset пока не скачан. Можно повторить с `--download-missing`.");
    }
    if status == AdapterStatus::BlockedUnsupportedDataset {
        println!(
            "Причина: {} сейчас не принимает dataset {} как канонический input без отдельного adapter/patch слоя.",
            benchmark.display_name, dataset.display_name
        );
    }
    if status == AdapterStatus::BlockedUpstreamDisabled {
        println!(
            "Причина: upstream ann-benchmarks сейчас держит canonical qdrant config как disabled=true. Amai честно не называет такой contour prepared, пока upstream default path не станет исполнимым или не появится отдельный override-policy."
        );
    }
    if status == AdapterStatus::BlockedConversionRequired {
        println!(
            "Причина: VectorDBBench custom dataset path требует Parquet bundle `train/test/neighbors`, а текущий dataset в HDF5. Runner уже materialized fail-closed и не притворяется прямой совместимостью."
        );
        if !dataset_path.exists() {
            println!("Дополнительно: исходный HDF5 dataset тоже пока не скачан.");
        }
    }
    println!();
    println!("Сравнивать рядом с внутренним Amai contour:");
    for command in comparison_commands {
        println!("- {}", command);
    }
    Ok(())
}

pub fn print_external_harvest(
    repo_root: &Path,
    benchmark_query: &str,
    dataset_query: &str,
    output_dir_override: Option<&Path>,
) -> Result<()> {
    let registry = load_registry(repo_root)?;
    let catalog = load_dataset_catalog(repo_root)?;
    let (benchmark_code, benchmark) = resolve_benchmark(&registry, benchmark_query)
        .ok_or_else(|| anyhow!("unknown external benchmark: {benchmark_query}"))?;
    let (dataset_code, dataset) = resolve_dataset(&catalog, dataset_query)
        .ok_or_else(|| anyhow!("unknown external dataset: {dataset_query}"))?;
    let output_dir = output_dir_override
        .map(|path| path.to_path_buf())
        .unwrap_or_else(|| {
            repo_root
                .join("state")
                .join("external-benchmarks")
                .join("runs")
                .join(benchmark_code)
                .join(dataset_code)
                .join("latest")
        });
    let summary_path = output_dir.join("summary.json");
    let report_path = output_dir.join("report.md");
    let script_path = output_dir.join("run_external.sh");
    let status_path = output_dir.join("run_status.json");
    let log_path = output_dir.join("run_external.log");
    let summary_text = fs::read_to_string(&summary_path)
        .with_context(|| format!("failed to read {}", summary_path.display()))?;
    let summary_json: Value = serde_json::from_str(&summary_text)
        .with_context(|| format!("failed to parse {}", summary_path.display()))?;
    let run_status = if status_path.exists() {
        let raw = fs::read_to_string(&status_path)
            .with_context(|| format!("failed to read {}", status_path.display()))?;
        Some(
            serde_json::from_str::<Value>(&raw)
                .with_context(|| format!("failed to parse {}", status_path.display()))?,
        )
    } else {
        None
    };
    let log_size = if log_path.exists() {
        Some(
            fs::metadata(&log_path)
                .with_context(|| format!("failed to stat {}", log_path.display()))?
                .len(),
        )
    } else {
        None
    };
    let external_results = collect_external_result_summaries(&output_dir)?;
    let run_status = reconcile_run_status_with_runtime(
        run_status,
        &external_results,
        Some(&output_dir),
        summary_json["benchmark_qdrant_http_url"].as_str(),
    );

    println!("Amai external benchmark harvest");
    println!();
    println!("Benchmark: {} ({})", benchmark.display_name, benchmark_code);
    println!("Dataset: {} ({})", dataset.display_name, dataset_code);
    println!(
        "Adapter status: {}",
        summary_json["status"].as_str().unwrap_or("unknown")
    );
    println!(
        "Adapter kind: {}",
        summary_json["adapter_kind"].as_str().unwrap_or("unknown")
    );
    println!("Workspace: {}", output_dir.display());
    println!("Summary: {}", summary_path.display());
    println!(
        "Artifacts: report={} script={} status={} log={}",
        if report_path.exists() { "yes" } else { "no" },
        if script_path.exists() { "yes" } else { "no" },
        if status_path.exists() { "yes" } else { "no" },
        if log_path.exists() { "yes" } else { "no" }
    );
    if let Some(bundle_dir) = summary_json["conversion_bundle"]["bundle_dir"].as_str() {
        println!("Conversion bundle: {}", bundle_dir);
    }
    if let Some(run_status) = &run_status {
        println!(
            "Run state: {}",
            run_status["state"].as_str().unwrap_or("unknown")
        );
        if let Some(exit_code) = run_status["exit_code"].as_i64() {
            println!("Exit code: {}", exit_code);
        }
        if let Some(message) = run_status["message"].as_str() {
            println!("Message: {}", message);
        }
        if let Some(result_verdict) = run_status["result_verdict"].as_str() {
            println!("Result verdict: {}", result_verdict);
        }
        if let Some(finished_at) = run_status["finished_at_epoch_s"].as_u64() {
            println!("Finished at epoch_s: {}", finished_at);
        }
    } else {
        println!("Run state: not_started");
    }
    if let Some(bytes) = log_size {
        println!("Log size: {}", format_bytes(bytes));
    }
    if external_results.is_empty() {
        println!("External result files: ещё не появились");
    } else {
        println!("External result files: {}", external_results.len());
        for result in &external_results {
            println!(
                "- {} :: run_id={} :: db={} :: task_label={} :: label={} :: qps={} :: recall={} :: p99={} ms :: p95={} ms :: load={} s :: max_load_count={}",
                result.path.display(),
                result.run_id,
                result.db,
                result.task_label,
                result.label,
                format_optional_f64(result.qps, 4),
                format_optional_f64(result.recall, 4),
                format_optional_f64(result.serial_latency_p99, 4),
                format_optional_f64(result.serial_latency_p95, 4),
                format_optional_f64(result.load_duration, 4),
                format_optional_f64(result.max_load_count, 4),
            );
        }
    }
    if let Some(overrides) = summary_json["compatibility_overrides"].as_array()
        && !overrides.is_empty()
    {
        println!();
        println!("Amai local compatibility overrides:");
        for item in overrides.iter().filter_map(|value| value.as_str()) {
            println!("- {}", item);
        }
    }
    println!();
    println!("Сравнивать рядом с внутренним Amai contour:");
    for command in summary_json["comparison_commands"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item.as_str())
    {
        println!("- {}", command);
    }
    Ok(())
}

pub(crate) fn benchmark_run_active_for_qdrant_http_url(
    repo_root: &Path,
    qdrant_http_url: &str,
) -> Option<bool> {
    let output_dir = latest_matching_output_dir_for_qdrant_http_url(repo_root, qdrant_http_url)?;
    let summary_path = output_dir.join("summary.json");
    let summary_text = fs::read_to_string(&summary_path).ok()?;
    let summary_json: Value = serde_json::from_str(&summary_text).ok()?;
    let status_path = output_dir.join("run_status.json");
    let run_status = if status_path.exists() {
        let raw = fs::read_to_string(&status_path).ok()?;
        serde_json::from_str::<Value>(&raw).ok()
    } else {
        None
    };
    let external_results = collect_external_result_summaries(&output_dir).ok()?;
    let run_status = reconcile_run_status_with_runtime(
        run_status,
        &external_results,
        Some(&output_dir),
        summary_json["benchmark_qdrant_http_url"].as_str(),
    );
    Some(
        run_status
            .as_ref()
            .and_then(|value| value["state"].as_str())
            == Some("running"),
    )
}

fn ordered_benchmarks(registry: &ExternalBenchmarkFile) -> Vec<(&String, &ExternalBenchmarkEntry)> {
    let mut entries = registry.benchmarks.iter().collect::<Vec<_>>();
    entries.sort_by_key(|(code, entry)| (entry.order, *code));
    entries
}

fn ordered_datasets(catalog: &ExternalDatasetFile) -> Vec<(&String, &ExternalDatasetEntry)> {
    let mut entries = catalog.datasets.iter().collect::<Vec<_>>();
    entries.sort_by_key(|(code, entry)| (entry.order, *code));
    entries
}

fn resolve_dataset<'a>(
    catalog: &'a ExternalDatasetFile,
    dataset_query: &str,
) -> Option<(&'a str, &'a ExternalDatasetEntry)> {
    if let Some(entry) = catalog.datasets.get_key_value(dataset_query) {
        return Some((entry.0.as_str(), entry.1));
    }
    let query = normalize_key(dataset_query);
    catalog
        .datasets
        .iter()
        .find(|(code, dataset)| {
            normalize_key(code) == query
                || normalize_key(&dataset.display_name) == query
                || dataset
                    .aliases
                    .iter()
                    .any(|alias| normalize_key(alias) == query)
        })
        .map(|(code, dataset)| (code.as_str(), dataset))
}

fn resolve_benchmark<'a>(
    registry: &'a ExternalBenchmarkFile,
    benchmark_query: &str,
) -> Option<(&'a str, &'a ExternalBenchmarkEntry)> {
    if let Some(entry) = registry.benchmarks.get_key_value(benchmark_query) {
        return Some((entry.0.as_str(), entry.1));
    }
    let query = normalize_key(benchmark_query);
    registry
        .benchmarks
        .iter()
        .find(|(code, entry)| {
            normalize_key(code) == query
                || normalize_key(&entry.display_name) == query
                || entry
                    .aliases
                    .iter()
                    .any(|alias| normalize_key(alias) == query)
        })
        .map(|(code, entry)| (code.as_str(), entry))
}

fn normalize_key(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(|ch| ch.to_lowercase())
        .collect()
}

fn recommended_datasets<'a>(
    benchmark_code: &str,
    catalog: &'a ExternalDatasetFile,
) -> Vec<(&'a String, &'a ExternalDatasetEntry)> {
    let mut entries = catalog
        .datasets
        .iter()
        .filter(|(_, entry)| {
            entry
                .usage_scope
                .iter()
                .any(|scope| normalize_key(scope) == normalize_key(benchmark_code))
        })
        .collect::<Vec<_>>();
    entries.sort_by_key(|(code, entry)| (entry.order, *code));
    entries
}

fn benchmark_supports_dataset(benchmark_code: &str, dataset: &ExternalDatasetEntry) -> bool {
    dataset
        .usage_scope
        .iter()
        .any(|scope| normalize_key(scope) == normalize_key(benchmark_code))
}

fn adapter_kind_for(benchmark_code: &str) -> &'static str {
    if benchmark_code == "vectordbbench" {
        "custom_parquet_bundle"
    } else {
        "direct_hdf5"
    }
}

fn determine_adapter_status(
    benchmark_code: &str,
    dataset: &ExternalDatasetEntry,
    dataset_exists: bool,
    upstream_dir: &Path,
    bundle_ready: bool,
) -> AdapterStatus {
    if !benchmark_supports_dataset(benchmark_code, dataset) {
        AdapterStatus::BlockedUnsupportedDataset
    } else if upstream_disables_default_launch(benchmark_code, upstream_dir) {
        AdapterStatus::BlockedUpstreamDisabled
    } else if benchmark_code == "vectordbbench" && !bundle_ready {
        AdapterStatus::BlockedConversionRequired
    } else if !dataset_exists {
        AdapterStatus::BlockedDatasetMissing
    } else {
        AdapterStatus::Prepared
    }
}

fn upstream_disables_default_launch(benchmark_code: &str, upstream_dir: &Path) -> bool {
    if benchmark_code != "ann_benchmarks" {
        return false;
    }
    let qdrant_config = upstream_dir
        .join("ann_benchmarks")
        .join("algorithms")
        .join("qdrant")
        .join("config.yml");
    let Ok(content) = fs::read_to_string(&qdrant_config) else {
        return false;
    };
    content.lines().any(|line| line.trim() == "disabled: true")
}

fn build_launch_commands(
    benchmark_code: &str,
    upstream_dir: &Path,
    dataset_path: &Path,
    dataset: &ExternalDatasetEntry,
    output_dir: &Path,
    converted_bundle: Option<&VectorDbBenchBundle>,
    benchmark_qdrant_http_url: &str,
) -> Vec<String> {
    match benchmark_code {
        "ann_benchmarks" => {
            let dataset_name = ann_benchmark_dataset_name(dataset);
            let linked_dataset_path = upstream_dir
                .join("data")
                .join(format!("{dataset_name}.hdf5"));
            vec![
                format!(
                    "if [ ! -d {git_dir} ]; then git clone https://github.com/erikbern/ann-benchmarks.git {clone_dir}; fi",
                    git_dir = shell_quote(&upstream_dir.join(".git").display().to_string()),
                    clone_dir = shell_quote(&upstream_dir.display().to_string()),
                ),
                format!("cd {}", shell_quote(&upstream_dir.display().to_string())),
                "if [ ! -x ./.venv/bin/python3 ]; then python3 -m venv .venv; fi".to_owned(),
                "if [ ! -x ./.venv/bin/python ]; then ln -sf python3 ./.venv/bin/python; fi"
                    .to_owned(),
                "if [ ! -x ./.venv/bin/run.py ] && [ ! -f ./.venv/.amai-ann-ready ]; then ./.venv/bin/pip install -r requirements.txt && touch ./.venv/.amai-ann-ready; fi".to_owned(),
                "mkdir -p data".to_owned(),
                format!(
                    "ln -sf {source} {target}",
                    source = shell_quote(&dataset_path.display().to_string()),
                    target = shell_quote(&linked_dataset_path.display().to_string()),
                ),
                "./.venv/bin/python install.py --algorithm qdrant".to_owned(),
                format!(
                    "./.venv/bin/python run.py --dataset {dataset_name} --algorithm qdrant --runs 1 --parallelism 1 --force"
                ),
            ]
        }
        "vectordbbench" => {
            let Some(bundle) = converted_bundle else {
                return vec![format!(
                    "# bundle not ready yet for {} from {}",
                    dataset.display_name,
                    dataset_path.display()
                )];
            };
            let case_name = format!("Amai{}", dataset_code_slug(&bundle.dataset_dir));
            let qdrant_timeout_marker = vectordbbench_qdrant_timeout_marker();
            let qdrant_client_marker = vectordbbench_qdrant_client_marker();
            let qdrant_container_name = format!(
                "amai-vdbbench-qdrant-{}",
                dataset_code_slug(&bundle.dataset_dir)
            );
            let mut commands = vec![
                format!(
                    "if [ ! -d {git_dir} ]; then git clone https://github.com/zilliztech/VectorDBBench.git {clone_dir}; fi",
                    git_dir = shell_quote(&upstream_dir.join(".git").display().to_string()),
                    clone_dir = shell_quote(&upstream_dir.display().to_string()),
                ),
                format!("cd {}", shell_quote(&upstream_dir.display().to_string())),
                "if [ ! -x ./.venv/bin/python3 ]; then python3 -m venv .venv; fi".to_owned(),
                "export AMAI_VDBBENCH_REINSTALL=0".to_owned(),
            ];
            commands.extend(vectordbbench_qdrant_timeout_patch_commands());
            commands.extend(vec![
                format!(
                    "if [ ! -x ./.venv/bin/vectordbbench ] || [ ! -f ./.venv/.amai-vdbbench-ready ] || [ \"$AMAI_VDBBENCH_REINSTALL\" = \"1\" ] || [ ! -f {patch_marker} ] || [ ! -f {client_marker} ]; then ./.venv/bin/pip install '.[qdrant]' && ./.venv/bin/pip install 'qdrant-client=={client_version}' && touch ./.venv/.amai-vdbbench-ready && touch {client_marker}; fi",
                    patch_marker = shell_quote(&qdrant_timeout_marker),
                    client_marker = shell_quote(&qdrant_client_marker),
                    client_version = AMAI_VDBBENCH_QDRANT_CLIENT_VERSION,
                ),
                format!(
                    "export AMAI_BENCHMARK_QDRANT_HTTP_URL_EFFECTIVE={}",
                    shell_quote(benchmark_qdrant_http_url)
                ),
                format!(
                    "export AMAI_VDBBENCH_QDRANT_CONTAINER_NAME={}",
                    shell_quote(&qdrant_container_name)
                ),
                "export AMAI_VDBBENCH_QDRANT_DOCKER_BIND=\"$(python3 - \"$AMAI_BENCHMARK_QDRANT_HTTP_URL_EFFECTIVE\" <<'PY'\nfrom urllib.parse import urlparse\nimport sys\nurl = urlparse(sys.argv[1])\nhost = url.hostname or '127.0.0.1'\nport = url.port or 6333\nprint(f'{host}:{port}')\nPY\n)\"".to_owned(),
                "cleanup_qdrant_container(){ docker rm -f \"$AMAI_VDBBENCH_QDRANT_CONTAINER_NAME\" >/dev/null 2>&1 || true; }".to_owned(),
                "cleanup_qdrant_container".to_owned(),
                "trap cleanup_qdrant_container EXIT".to_owned(),
                format!(
                    "docker run -d --rm --name \"$AMAI_VDBBENCH_QDRANT_CONTAINER_NAME\" -p \"$AMAI_VDBBENCH_QDRANT_DOCKER_BIND:6333\" {image} >/dev/null",
                    image = AMAI_VDBBENCH_QDRANT_IMAGE
                ),
                "python3 - \"$AMAI_BENCHMARK_QDRANT_HTTP_URL_EFFECTIVE\" <<'PY'\nimport sys\nimport time\nimport urllib.request\nurl = sys.argv[1].rstrip('/') + '/metrics'\nlast_error = None\ndeadline = time.time() + 120\nwhile time.time() < deadline:\n    try:\n        urllib.request.urlopen(url, timeout=5).read(64)\n        raise SystemExit(0)\n    except Exception as exc:\n        last_error = exc\n        time.sleep(1)\nraise SystemExit(f'benchmark qdrant did not become ready at {url}: {last_error}')\nPY".to_owned(),
                format!(
                    "export DATASET_LOCAL_DIR={}",
                    shell_quote(&bundle.dataset_root.display().to_string())
                ),
                format!(
                    "export RESULTS_LOCAL_DIR={}",
                    shell_quote(
                        &output_dir
                            .join("vectordbbench-results")
                            .display()
                            .to_string()
                    )
                ),
                "rm -rf \"$RESULTS_LOCAL_DIR\"".to_owned(),
                "mkdir -p \"$RESULTS_LOCAL_DIR\"".to_owned(),
                format!(
                    "export LOG_FILE={}",
                    shell_quote(
                        &output_dir
                            .join("vectordbbench-runtime.log")
                            .display()
                            .to_string()
                    )
                ),
                format!(
                    "./.venv/bin/vectordbbench qdrantlocal --url \"$AMAI_BENCHMARK_QDRANT_HTTP_URL_EFFECTIVE\" --timeout {timeout} --case-type PerformanceCustomDataset --custom-case-name {case_name} --custom-case-description 'Amai external VectorDBBench contour' --custom-dataset-name {dataset_name} --custom-dataset-dir {dataset_dir} --custom-dataset-size {dataset_size} --custom-dataset-dim {dataset_dim} --custom-dataset-metric-type {metric_type} --custom-dataset-file-count {file_count} --skip-custom-dataset-use-shuffled --custom-dataset-with-gt --drop-old --load --search-serial --skip-search-concurrent",
                    timeout = AMAI_VDBBENCH_QDRANT_TIMEOUT_SECONDS,
                    case_name = shell_quote(&case_name),
                    dataset_name = shell_quote(&bundle.dataset_name),
                    dataset_dir = shell_quote(&bundle.dataset_dir),
                    dataset_size = bundle.train_rows,
                    dataset_dim = bundle.dim,
                    metric_type = bundle.metric_type,
                    file_count = bundle.train_file_count,
                ),
            ]);
            commands
        }
        _ => vec![format!(
            "# launch contract not defined for {}",
            benchmark_code
        )],
    }
}

fn adapter_compatibility_overrides(benchmark_code: &str) -> Vec<String> {
    match benchmark_code {
        "vectordbbench" => vec![
            format!(
                "QdrantLocal transport timeout < {} s -> Amai local compatibility patch forces timeout = {} s; case thresholds and dataset semantics unchanged.",
                AMAI_VDBBENCH_QDRANT_TIMEOUT_SECONDS, AMAI_VDBBENCH_QDRANT_TIMEOUT_SECONDS
            ),
            format!(
                "Python qdrant-client pinned to {} to stay within the QdrantLocal 1.12.x compatibility window used by the upstream benchmark path.",
                AMAI_VDBBENCH_QDRANT_CLIENT_VERSION
            ),
            format!(
                "Benchmark Qdrant URL is env-driven via AMAI_BENCHMARK_QDRANT_HTTP_URL; fallback stays {}.",
                AMAI_VDBBENCH_QDRANT_HTTP_URL_FALLBACK
            ),
            format!(
                "Amai starts a dedicated Docker container {} for VectorDBBench so the external Qdrant never reuses unrelated project ports or state.",
                AMAI_VDBBENCH_QDRANT_IMAGE
            ),
        ],
        _ => Vec::new(),
    }
}

fn vectordbbench_qdrant_timeout_marker() -> String {
    format!(
        ".amai-vdbbench-qdrant-timeout-{}",
        AMAI_VDBBENCH_QDRANT_TIMEOUT_SECONDS
    )
}

fn vectordbbench_qdrant_client_marker() -> String {
    format!(
        ".amai-vdbbench-qdrant-client-{}",
        AMAI_VDBBENCH_QDRANT_CLIENT_VERSION
    )
}

fn vectordbbench_qdrant_timeout_patch_commands() -> Vec<String> {
    let marker = vectordbbench_qdrant_timeout_marker();
    vec![format!(
        r#"if [ ! -f {marker_path} ]; then
python3 - <<'PY'
from pathlib import Path

timeout = {timeout}
marker = Path("{marker_name}")

config_path = Path("vectordb_bench/backend/clients/qdrant_local/config.py")
config_text = config_path.read_text()
config_field = "class QdrantLocalConfig(DBConfig):\n    url: SecretStr\n"
config_field_with_timeout = "class QdrantLocalConfig(DBConfig):\n    url: SecretStr\n    timeout: int | None = None\n"
if "timeout: int | None = None" not in config_text:
    if config_field not in config_text:
        raise SystemExit("unexpected QdrantLocalConfig layout")
    config_text = config_text.replace(config_field, config_field_with_timeout, 1)
config_return = "    def to_dict(self) -> dict:\n        return {{\n            \"url\": self.url.get_secret_value(),\n        }}\n"
config_return_with_timeout = "    def to_dict(self) -> dict:\n        config = {{\n            \"url\": self.url.get_secret_value(),\n        }}\n        if self.timeout is not None:\n            config[\"timeout\"] = self.timeout\n        return config\n"
if "config[\"timeout\"] = self.timeout" not in config_text:
    if config_return not in config_text:
        raise SystemExit("unexpected QdrantLocalConfig.to_dict layout")
    config_text = config_text.replace(config_return, config_return_with_timeout, 1)
config_path.write_text(config_text)

cli_path = Path("vectordb_bench/backend/clients/qdrant_local/cli.py")
cli_text = cli_path.read_text()
url_field = """    url: Annotated[
        str,
        click.option(\"--url\", type=str, help=\"Qdrant url\", required=True),
    ]
"""
timeout_field = """    timeout: Annotated[
        int,
        click.option(\"--timeout\", type=int, default={timeout}, help=\"Qdrant client transport timeout in seconds\"),
    ]
"""
if "click.option(\"--timeout\"" not in cli_text:
    if url_field not in cli_text:
        raise SystemExit("unexpected qdrant_local cli layout")
    cli_text = cli_text.replace(url_field, url_field + timeout_field, 1)
config_ctor = "        db_config=QdrantLocalConfig(url=SecretStr(parameters[\"url\"])),\n"
config_ctor_with_timeout = "        db_config=QdrantLocalConfig(url=SecretStr(parameters[\"url\"]), timeout=parameters[\"timeout\"]),\n"
if "timeout=parameters[\"timeout\"]" not in cli_text:
    if config_ctor not in cli_text:
        raise SystemExit("unexpected qdrant_local cli ctor layout")
    cli_text = cli_text.replace(config_ctor, config_ctor_with_timeout, 1)
cli_path.write_text(cli_text)

marker.write_text("patched\n")
PY
export AMAI_VDBBENCH_REINSTALL=1
fi"#,
        marker_path = shell_quote(&marker),
        marker_name = marker,
        timeout = AMAI_VDBBENCH_QDRANT_TIMEOUT_SECONDS,
    )]
}

fn dataset_code_slug(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .collect()
}

fn adapter_status_code(status: AdapterStatus) -> &'static str {
    match status {
        AdapterStatus::Prepared => "prepared",
        AdapterStatus::BlockedUnsupportedDataset => "blocked_unsupported_dataset",
        AdapterStatus::BlockedUpstreamDisabled => "blocked_upstream_disabled",
        AdapterStatus::BlockedConversionRequired => "blocked_conversion_required",
        AdapterStatus::BlockedDatasetMissing => "blocked_dataset_missing",
    }
}

fn adapter_status_label(status: AdapterStatus) -> &'static str {
    match status {
        AdapterStatus::Prepared => "готов к следующему запуску",
        AdapterStatus::BlockedUnsupportedDataset => "dataset пока не поддержан этим benchmark-ом",
        AdapterStatus::BlockedUpstreamDisabled => {
            "upstream benchmark сейчас отключил этот launch path"
        }
        AdapterStatus::BlockedConversionRequired => "нужна конвертация dataset",
        AdapterStatus::BlockedDatasetMissing => "dataset ещё не скачан",
    }
}

fn render_adapter_report(ctx: &AdapterRenderContext<'_>) -> String {
    let captured_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!(
        "# Amai External Adapter Report\n\n\
captured_at_epoch_s: {captured_at}\n\n\
## Benchmark\n\n\
- code: `{benchmark_code}`\n\
- name: `{benchmark_name}`\n\
- type: `{benchmark_type}`\n\n\
## Dataset\n\n\
- code: `{dataset_code}`\n\
- name: `{dataset_name}`\n\
- format: `{dataset_family}`\n\
- distance: `{dataset_distance}`\n\
- dimensions: `{dataset_dimensions}`\n\
- local path: `{dataset_path}`\n\n\
## Adapter Status\n\n\
- status: `{status_code}`\n\
- label: {status_label}\n\
- adapter_kind: `{adapter_kind}`\n\n\
## Amai Local Compatibility Overrides\n\n\
{compatibility_overrides_block}\
## Launch Commands\n\n\
{launch_commands_block}\n\
## Internal Amai Comparison Commands\n\n\
{comparison_commands_block}\n",
        benchmark_code = ctx.benchmark_code,
        benchmark_name = ctx.benchmark.display_name,
        benchmark_type = ctx.benchmark.benchmark_kind,
        dataset_code = ctx.dataset_code,
        dataset_name = ctx.dataset.display_name,
        dataset_family = ctx.dataset.family,
        dataset_distance = ctx.dataset.distance,
        dataset_dimensions = ctx.dataset.dimensions,
        dataset_path = ctx.dataset_path.display(),
        status_code = adapter_status_code(ctx.status),
        status_label = adapter_status_label(ctx.status),
        adapter_kind = ctx.adapter_kind,
        compatibility_overrides_block = if ctx.compatibility_overrides.is_empty() {
            "- none\n\n".to_owned()
        } else {
            ctx.compatibility_overrides
                .iter()
                .map(|item| format!("- {item}\n"))
                .collect::<String>()
                + "\n"
        },
        launch_commands_block = ctx
            .launch_commands
            .iter()
            .map(|cmd| format!("- `{cmd}`\n"))
            .collect::<String>(),
        comparison_commands_block = ctx
            .comparison_commands
            .iter()
            .map(|cmd| format!("- `{cmd}`\n"))
            .collect::<String>(),
    )
}

fn render_adapter_script(ctx: &AdapterRenderContext<'_>) -> String {
    let body = if ctx.status == AdapterStatus::Prepared {
        format!(
            "SCRIPT_DIR=\"$(cd -- \"$(dirname -- \"${{BASH_SOURCE[0]}}\")\" && pwd)\"\n\
LOG_PATH=\"$SCRIPT_DIR/run_external.log\"\n\
STATUS_PATH=\"$SCRIPT_DIR/run_status.json\"\n\
STARTED_AT=\"$(date +%s)\"\n\
printf '{{\"state\":\"running\",\"exit_code\":null,\"message\":\"upstream launch started\",\"started_at_epoch_s\":%s,\"runner_pid\":%s}}\\n' \"$STARTED_AT\" \"$$\" > \"$STATUS_PATH\"\n\
echo \"Amai external benchmark launch: {benchmark} / {dataset}\" | tee \"$LOG_PATH\"\n\
echo \"Источник: подготовленный adapter workspace; запускается реальный upstream path, а не echo-заглушка.\" | tee -a \"$LOG_PATH\"\n\
set +e\n\
(\n\
set -euo pipefail\n\
{}\n\
) 2>&1 | tee -a \"$LOG_PATH\"\n\
CMD_EXIT=${{PIPESTATUS[0]}}\n\
set -e\n\
FINISHED_AT=\"$(date +%s)\"\n\
RESULT_VERDICT=\"$(python3 - \"$SCRIPT_DIR\" <<'PY'\n\
import json\n\
import pathlib\n\
import sys\n\
\n\
root = pathlib.Path(sys.argv[1])\n\
files = sorted(root.rglob('result_*.json'))\n\
if not files:\n\
    print('no_results')\n\
    raise SystemExit(0)\n\
for path in files:\n\
    data = json.loads(path.read_text())\n\
    for result in data.get('results', []):\n\
        if result.get('label') != ':)':\n\
            print('benchmark_failed')\n\
            raise SystemExit(0)\n\
print('benchmark_ok')\n\
PY\n\
)\"\n\
if [ \"$CMD_EXIT\" -eq 0 ] && [ \"$RESULT_VERDICT\" = \"benchmark_ok\" ]; then\n\
  printf '{{\"state\":\"finished_ok\",\"exit_code\":%s,\"message\":\"upstream launch finished successfully\",\"started_at_epoch_s\":%s,\"finished_at_epoch_s\":%s,\"result_verdict\":\"%s\",\"runner_pid\":%s}}\\n' \"$CMD_EXIT\" \"$STARTED_AT\" \"$FINISHED_AT\" \"$RESULT_VERDICT\" \"$$\" > \"$STATUS_PATH\"\n\
elif [ \"$CMD_EXIT\" -eq 0 ] && [ \"$RESULT_VERDICT\" = \"benchmark_failed\" ]; then\n\
  printf '{{\"state\":\"finished_benchmark_failed\",\"exit_code\":%s,\"message\":\"upstream launch returned exit 0 but benchmark result label is not normal\",\"started_at_epoch_s\":%s,\"finished_at_epoch_s\":%s,\"result_verdict\":\"%s\",\"runner_pid\":%s}}\\n' \"$CMD_EXIT\" \"$STARTED_AT\" \"$FINISHED_AT\" \"$RESULT_VERDICT\" \"$$\" > \"$STATUS_PATH\"\n\
elif [ \"$CMD_EXIT\" -eq 0 ] && [ \"$RESULT_VERDICT\" = \"no_results\" ]; then\n\
  printf '{{\"state\":\"finished_without_results\",\"exit_code\":%s,\"message\":\"upstream launch returned exit 0 but did not produce result files\",\"started_at_epoch_s\":%s,\"finished_at_epoch_s\":%s,\"result_verdict\":\"%s\",\"runner_pid\":%s}}\\n' \"$CMD_EXIT\" \"$STARTED_AT\" \"$FINISHED_AT\" \"$RESULT_VERDICT\" \"$$\" > \"$STATUS_PATH\"\n\
else\n\
  printf '{{\"state\":\"finished_error\",\"exit_code\":%s,\"message\":\"upstream launch finished with error\",\"started_at_epoch_s\":%s,\"finished_at_epoch_s\":%s,\"result_verdict\":\"%s\",\"runner_pid\":%s}}\\n' \"$CMD_EXIT\" \"$STARTED_AT\" \"$FINISHED_AT\" \"$RESULT_VERDICT\" \"$$\" > \"$STATUS_PATH\"\n\
fi\n\
exit \"$CMD_EXIT\"\n",
            ctx.launch_commands.join("\n"),
            benchmark = shell_escape_echo(ctx.benchmark.display_name.as_str()),
            dataset = shell_escape_echo(ctx.dataset.display_name.as_str()),
        )
    } else if ctx.status == AdapterStatus::BlockedDatasetMissing {
        format!(
            "echo \"Dataset ещё не скачан: {}\"\nexit 2\n",
            ctx.dataset_path.display()
        )
    } else if ctx.status == AdapterStatus::BlockedUnsupportedDataset {
        format!(
            "echo \"Dataset {} пока не поддержан benchmark-ом {} без отдельного adapter/patch слоя.\"\n\
echo \"Исходный файл: {}\"\n\
echo \"Upstream repo: {}\"\n\
exit 4\n",
            ctx.dataset.display_name,
            ctx.benchmark.display_name,
            ctx.dataset_path.display(),
            ctx.upstream_dir.display()
        )
    } else if ctx.status == AdapterStatus::BlockedUpstreamDisabled {
        format!(
            "echo \"Upstream {} сейчас держит canonical qdrant config в disabled=true; default launch path не должен считаться готовым.\"\n\
echo \"Upstream repo: {}\"\n\
echo \"Чтобы идти дальше честно, нужен либо upstream enable, либо отдельный explicit override-policy.\"\n\
exit 5\n",
            ctx.benchmark.display_name,
            ctx.upstream_dir.display(),
        )
    } else {
        format!(
            "echo \"Для {} dataset {} пока не запускается напрямую: нужен Parquet bundle train/test/neighbors вместо HDF5.\"\n\
echo \"Исходный файл: {}\"\n\
echo \"Upstream repo: {}\"\n\
exit 3\n",
            ctx.benchmark.display_name,
            ctx.dataset.display_name,
            ctx.dataset_path.display(),
            ctx.upstream_dir.display()
        )
    };
    format!(
        "#!/usr/bin/env bash\nset -euo pipefail\n\n# benchmark: {benchmark_code}\n# benchmark_name: {benchmark_name}\n# dataset: {dataset_name}\n# adapter_kind: {adapter_kind}\n\n{body}",
        benchmark_code = ctx.benchmark_code,
        benchmark_name = ctx.benchmark.display_name,
        dataset_name = ctx.dataset.display_name,
        adapter_kind = ctx.adapter_kind
    )
}

fn shell_escape_echo(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn ann_benchmark_dataset_name(dataset: &ExternalDatasetEntry) -> String {
    dataset
        .local_filename
        .strip_suffix(".hdf5")
        .unwrap_or(dataset.display_name.as_str())
        .to_owned()
}

fn inspect_tool(tool: &str) -> ToolCheck {
    let version_args = match tool {
        "python3" => vec!["--version"],
        "docker" => vec!["--version"],
        "git" => vec!["--version"],
        _ => vec!["--version"],
    };
    match Command::new(tool).args(version_args).output() {
        Ok(output) if output.status.success() => {
            let version = first_line_lossy(&output.stdout, &output.stderr);
            ToolCheck {
                available: true,
                version,
            }
        }
        _ => ToolCheck {
            available: false,
            version: "нет данных".to_owned(),
        },
    }
}

fn inspect_upstream(url: &str) -> UpstreamCheck {
    match Command::new("git")
        .args(["ls-remote", url, "HEAD"])
        .output()
    {
        Ok(output) if output.status.success() => {
            let line = first_line_lossy(&output.stdout, &output.stderr);
            let head = line.split_whitespace().next().unwrap_or("HEAD").to_owned();
            UpstreamCheck {
                reachable: true,
                head,
            }
        }
        _ => UpstreamCheck {
            reachable: false,
            head: "HEAD".to_owned(),
        },
    }
}

fn command_ok(cmd: &str, args: &[&str]) -> bool {
    Command::new(cmd)
        .args(args)
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn first_line_lossy(stdout: &[u8], stderr: &[u8]) -> String {
    let stdout = String::from_utf8_lossy(stdout);
    let stderr = String::from_utf8_lossy(stderr);
    stdout
        .lines()
        .find(|line| !line.trim().is_empty())
        .or_else(|| stderr.lines().find(|line| !line.trim().is_empty()))
        .unwrap_or("нет данных")
        .trim()
        .to_owned()
}

fn collect_external_result_summaries(output_dir: &Path) -> Result<Vec<ExternalResultSummary>> {
    let mut stack = vec![output_dir.to_path_buf()];
    let mut files = Vec::new();
    while let Some(dir) = stack.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries {
            let entry =
                entry.with_context(|| format!("failed to read entry in {}", dir.display()))?;
            let path = entry.path();
            let file_type = entry
                .file_type()
                .with_context(|| format!("failed to stat {}", path.display()))?;
            if file_type.is_dir() {
                stack.push(path);
                continue;
            }
            let file_name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default();
            if file_name.starts_with("result_") && file_name.ends_with(".json") {
                files.push(path);
            }
        }
    }
    files.sort();
    let mut summaries = Vec::new();
    for file in files {
        summaries.push(parse_external_result_summary(&file)?);
    }
    Ok(summaries)
}

fn parse_external_result_summary(path: &Path) -> Result<ExternalResultSummary> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read external result {}", path.display()))?;
    let value: Value = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse external result {}", path.display()))?;
    let run_id = value["run_id"].as_str().unwrap_or("unknown").to_owned();
    let task_label = value["task_label"].as_str().unwrap_or("unknown").to_owned();
    let first_result = value["results"]
        .as_array()
        .and_then(|results| results.first())
        .ok_or_else(|| anyhow!("external result {} has no results[]", path.display()))?;
    let metrics = &first_result["metrics"];
    let task_config = &first_result["task_config"];
    let db = task_config["db"].as_str().unwrap_or("unknown").to_owned();
    let label = first_result["label"]
        .as_str()
        .unwrap_or("unknown")
        .to_owned();
    Ok(ExternalResultSummary {
        path: path.to_path_buf(),
        run_id,
        task_label,
        db,
        label,
        qps: metrics["qps"].as_f64(),
        serial_latency_p99: metrics["serial_latency_p99"].as_f64(),
        serial_latency_p95: metrics["serial_latency_p95"].as_f64(),
        recall: metrics["recall"].as_f64(),
        max_load_count: metrics["max_load_count"].as_f64(),
        load_duration: metrics["load_duration"].as_f64(),
    })
}

fn reconcile_run_status(
    run_status: Option<Value>,
    external_results: &[ExternalResultSummary],
) -> Option<Value> {
    let run_status = run_status?;
    if run_status["state"].as_str() != Some("running") {
        return Some(run_status);
    }
    if external_results.is_empty() {
        let runner_pid = run_status["runner_pid"]
            .as_u64()
            .and_then(|value| u32::try_from(value).ok());
        if let Some(runner_pid) = runner_pid
            && !process_is_alive(runner_pid)
        {
            return Some(json!({
                "state": "finished_error",
                "exit_code": run_status["exit_code"].clone(),
                "message": "runner pid is no longer alive and result files did not appear",
                "started_at_epoch_s": run_status["started_at_epoch_s"].clone(),
                "finished_at_epoch_s": now_epoch_s(),
                "result_verdict": "no_results",
                "runner_pid": runner_pid,
            }));
        }
        return Some(run_status);
    }
    let benchmark_failed = external_results.iter().any(|result| result.label != ":)");
    let result_verdict = if benchmark_failed {
        "benchmark_failed"
    } else {
        "benchmark_ok"
    };
    Some(json!({
        "state": if benchmark_failed { "finished_benchmark_failed" } else { "finished_ok" },
        "exit_code": run_status["exit_code"].clone(),
        "message": "result files already exist; stored run_status was stale",
        "started_at_epoch_s": run_status["started_at_epoch_s"].clone(),
        "finished_at_epoch_s": run_status["finished_at_epoch_s"].clone(),
        "result_verdict": result_verdict,
        "runner_pid": run_status["runner_pid"].clone(),
    }))
}

fn reconcile_run_status_with_runtime(
    run_status: Option<Value>,
    external_results: &[ExternalResultSummary],
    output_dir: Option<&Path>,
    benchmark_qdrant_http_url: Option<&str>,
) -> Option<Value> {
    let run_status = reconcile_run_status(run_status, external_results)?;
    if run_status["state"].as_str() != Some("running") {
        return Some(run_status);
    }
    let runner_pid = run_status["runner_pid"]
        .as_u64()
        .and_then(|value| u32::try_from(value).ok());
    let runner_alive = if let Some(runner_pid) = runner_pid {
        process_is_alive(runner_pid)
    } else {
        benchmark_runner_process_alive(output_dir, benchmark_qdrant_http_url)
    };
    if runner_alive {
        return Some(run_status);
    }
    Some(json!({
        "state": "finished_error",
        "exit_code": run_status["exit_code"].clone(),
        "message": "runner process is no longer alive and result files did not appear",
        "started_at_epoch_s": run_status["started_at_epoch_s"].clone(),
        "finished_at_epoch_s": now_epoch_s(),
        "result_verdict": "no_results",
        "runner_pid": run_status["runner_pid"].clone(),
    }))
}

fn latest_matching_output_dir_for_qdrant_http_url(
    repo_root: &Path,
    qdrant_http_url: &str,
) -> Option<PathBuf> {
    let runs_root = repo_root
        .join("state")
        .join("external-benchmarks")
        .join("runs");
    let mut best: Option<(u64, PathBuf)> = None;
    for benchmark_entry in fs::read_dir(&runs_root).ok()? {
        let benchmark_entry = benchmark_entry.ok()?;
        if !benchmark_entry.path().is_dir() {
            continue;
        }
        for dataset_entry in fs::read_dir(benchmark_entry.path()).ok()? {
            let dataset_entry = dataset_entry.ok()?;
            if !dataset_entry.path().is_dir() {
                continue;
            }
            let output_dir = dataset_entry.path().join("latest");
            let summary_path = output_dir.join("summary.json");
            if !summary_path.is_file() {
                continue;
            }
            let summary_text = fs::read_to_string(&summary_path).ok()?;
            let summary_json: Value = serde_json::from_str(&summary_text).ok()?;
            if summary_json["benchmark_qdrant_http_url"].as_str() != Some(qdrant_http_url) {
                continue;
            }
            let modified_rank = fs::metadata(&summary_path)
                .ok()?
                .modified()
                .ok()?
                .duration_since(UNIX_EPOCH)
                .ok()?
                .as_secs();
            match &best {
                Some((best_rank, _)) if *best_rank > modified_rank => {}
                _ => best = Some((modified_rank, output_dir)),
            }
        }
    }
    best.map(|(_, output_dir)| output_dir)
}

fn benchmark_runner_process_alive(
    output_dir: Option<&Path>,
    benchmark_qdrant_http_url: Option<&str>,
) -> bool {
    let script_marker = output_dir.map(|path| path.join("run_external.sh").display().to_string());
    let Some(proc_entries) = fs::read_dir("/proc").ok() else {
        return false;
    };
    for entry in proc_entries.flatten() {
        let file_name = entry.file_name();
        let Some(file_name) = file_name.to_str() else {
            continue;
        };
        if !file_name.chars().all(|ch| ch.is_ascii_digit()) {
            continue;
        }
        let Ok(cmdline) = fs::read(entry.path().join("cmdline")) else {
            continue;
        };
        if cmdline.is_empty() {
            continue;
        }
        let command = String::from_utf8_lossy(&cmdline).replace('\0', " ");
        if let Some(script_marker) = &script_marker
            && command.contains(script_marker)
        {
            return true;
        }
        if let Some(qdrant_http_url) = benchmark_qdrant_http_url
            && command.contains("vectordbbench")
            && command.contains(qdrant_http_url)
        {
            return true;
        }
    }
    false
}

fn process_is_alive(pid: u32) -> bool {
    Path::new("/proc").join(pid.to_string()).exists()
}

fn now_epoch_s() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn format_optional_f64(value: Option<f64>, precision: usize) -> String {
    match value {
        Some(value) => format!("{value:.precision$}"),
        None => "нет данных".to_owned(),
    }
}

fn load_registry(repo_root: &Path) -> Result<ExternalBenchmarkFile> {
    let path = registry_path(repo_root);
    let content = fs::read_to_string(&path).with_context(|| {
        format!(
            "failed to read external benchmark registry {}",
            path.display()
        )
    })?;
    toml::from_str(&content).context("failed to parse external benchmark registry")
}

fn registry_path(repo_root: &Path) -> std::path::PathBuf {
    repo_root.join("config/external_benchmark_targets.toml")
}

fn load_dataset_catalog(repo_root: &Path) -> Result<ExternalDatasetFile> {
    let path = dataset_catalog_path(repo_root);
    let content = fs::read_to_string(&path).with_context(|| {
        format!(
            "failed to read external benchmark dataset catalog {}",
            path.display()
        )
    })?;
    toml::from_str(&content).context("failed to parse external benchmark dataset catalog")
}

fn dataset_catalog_path(repo_root: &Path) -> PathBuf {
    repo_root.join("config/external_benchmark_datasets.toml")
}

fn dataset_root(repo_root: &Path, relative_root: &str) -> PathBuf {
    repo_root.join(relative_root)
}

fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let bytes_f = bytes as f64;
    if bytes_f >= GB {
        format!("{:.2} GiB", bytes_f / GB)
    } else if bytes_f >= MB {
        format!("{:.2} MiB", bytes_f / MB)
    } else if bytes_f >= KB {
        format!("{:.2} KiB", bytes_f / KB)
    } else {
        format!("{} B", bytes)
    }
}

async fn download_dataset_file(dataset: &ExternalDatasetEntry, path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        tokio_fs::create_dir_all(parent)
            .await
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let client = Client::new();
    let response = client
        .get(&dataset.download_url)
        .send()
        .await
        .with_context(|| format!("failed to request {}", dataset.download_url))?
        .error_for_status()
        .with_context(|| format!("download failed for {}", dataset.download_url))?;
    let temp_path = path.with_extension("part");
    let mut file = tokio_fs::File::create(&temp_path)
        .await
        .with_context(|| format!("failed to create {}", temp_path.display()))?;
    let mut response = response;
    while let Some(chunk) = response
        .chunk()
        .await
        .with_context(|| format!("failed while streaming {}", dataset.download_url))?
    {
        file.write_all(&chunk)
            .await
            .with_context(|| format!("failed to write {}", temp_path.display()))?;
    }
    file.flush()
        .await
        .with_context(|| format!("failed to flush {}", temp_path.display()))?;
    drop(file);
    tokio_fs::rename(&temp_path, path).await.with_context(|| {
        format!(
            "failed to rename {} -> {}",
            temp_path.display(),
            path.display()
        )
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        AdapterStatus, ExternalBenchmarkEntry, ExternalBenchmarkFile, ExternalBenchmarkSource,
        ExternalDatasetEntry, ExternalDatasetFile, ExternalDatasetStorage, ExternalResultSummary,
        VectorDbBenchBundle, adapter_compatibility_overrides, ann_benchmark_dataset_name,
        build_launch_commands, determine_adapter_status, normalize_key, ordered_benchmarks,
        recommended_datasets, reconcile_run_status, reconcile_run_status_with_runtime,
        resolve_benchmark, resolve_dataset,
    };
    use serde_json::json;
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::{Path, PathBuf};

    #[test]
    fn resolve_benchmark_accepts_aliases_and_display_name() {
        let registry = sample_registry();
        let (code, _) = resolve_benchmark(&registry, "VectorDBBench").expect("display name");
        assert_eq!(code, "vectordbbench");

        let (code, _) = resolve_benchmark(&registry, "ann benchmarks").expect("alias");
        assert_eq!(code, "ann_benchmarks");
    }

    #[test]
    fn ordered_benchmarks_sort_by_order_then_code() {
        let registry = sample_registry();
        let ordered = ordered_benchmarks(&registry);
        assert_eq!(ordered[0].0.as_str(), "vectordbbench");
        assert_eq!(ordered[1].0.as_str(), "ann_benchmarks");
    }

    #[test]
    fn normalize_key_drops_separators() {
        assert_eq!(normalize_key("ann-benchmarks"), "annbenchmarks");
        assert_eq!(normalize_key("Vector DB Bench"), "vectordbbench");
    }

    #[test]
    fn recommended_datasets_match_scope() {
        let catalog = sample_catalog();
        let datasets = recommended_datasets("ann_benchmarks", &catalog);
        assert_eq!(datasets.len(), 2);
        assert_eq!(datasets[0].0.as_str(), "dbpedia_openai_1000k_angular");
    }

    #[test]
    fn resolve_dataset_accepts_aliases() {
        let catalog = sample_catalog();
        let (code, _) = resolve_dataset(&catalog, "sift").expect("alias");
        assert_eq!(code, "sift_128_euclidean");
    }

    #[test]
    fn ann_launch_commands_use_safe_python_workflow_instead_of_docker_compose() {
        let catalog = sample_catalog();
        let dataset = &catalog.datasets["dbpedia_openai_1000k_angular"];
        let commands = build_launch_commands(
            "ann_benchmarks",
            Path::new("/tmp/ann_benchmarks"),
            Path::new("/tmp/datasets/dbpedia-openai-1000k-angular.hdf5"),
            dataset,
            Path::new("/tmp/output"),
            None,
            "http://127.0.0.1:7633",
        );
        let joined = commands.join("\n");
        assert!(joined.contains("python3 -m venv .venv"));
        assert!(joined.contains("./.venv/bin/pip install -r requirements.txt"));
        assert!(joined.contains("./.venv/bin/python install.py --algorithm qdrant"));
        assert!(joined.contains("./.venv/bin/python run.py --dataset dbpedia-openai-1000k-angular --algorithm qdrant --runs 1 --parallelism 1 --force"));
        assert!(!joined.contains("docker compose up"));
    }

    #[test]
    fn vectordbbench_launch_commands_include_timeout_and_patch_path() {
        let catalog = sample_catalog();
        let dataset = &catalog.datasets["dbpedia_openai_1000k_angular"];
        let bundle = VectorDbBenchBundle {
            dataset_root: Path::new("/tmp/bundle").to_path_buf(),
            bundle_dir: Path::new("/tmp/bundle/dbpedia").to_path_buf(),
            dataset_name: "AmaiExternal".to_owned(),
            dataset_dir: "dbpedia_openai_1000k_angular".to_owned(),
            train_rows: 990000,
            test_rows: 1000,
            neighbors_rows: 1000,
            dim: 1536,
            metric_type: "COSINE".to_owned(),
            train_file_count: 1,
            manifest_path: Path::new("/tmp/bundle/dbpedia/conversion_manifest.json").to_path_buf(),
        };
        let commands = build_launch_commands(
            "vectordbbench",
            Path::new("/tmp/vdbbench"),
            Path::new("/tmp/datasets/dbpedia-openai-1000k-angular.hdf5"),
            dataset,
            Path::new("/tmp/output"),
            Some(&bundle),
            "http://127.0.0.1:7633",
        );
        let joined = commands.join("\n");
        assert!(joined.contains("--timeout 600"));
        assert!(joined.contains("AMAI_VDBBENCH_REINSTALL=0"));
        assert!(joined.contains("click.option(\\\"--timeout\\\""));
        assert!(joined.contains("config[\\\"timeout\\\"] = self.timeout"));
        assert!(joined.contains("qdrant-client==1.12.2"));
        assert!(joined.contains("http://127.0.0.1:7633"));
        assert!(joined.contains("vectordbbench qdrantlocal --url"));
        assert!(
            joined.contains("docker run -d --rm --name \"$AMAI_VDBBENCH_QDRANT_CONTAINER_NAME\"")
        );
        assert!(joined.contains("qdrant/qdrant:v1.12.5"));
        assert!(joined.contains("benchmark qdrant did not become ready"));
    }

    #[test]
    fn vectordbbench_compatibility_overrides_include_client_pin_and_env_url() {
        let overrides = adapter_compatibility_overrides("vectordbbench");
        let joined = overrides.join("\n");
        assert!(joined.contains("qdrant-client pinned to 1.12.2"));
        assert!(joined.contains("AMAI_BENCHMARK_QDRANT_HTTP_URL"));
        assert!(joined.contains("http://127.0.0.1:6333"));
        assert!(joined.contains("qdrant/qdrant:v1.12.5"));
    }

    #[test]
    fn reconcile_run_status_marks_dead_runner_without_results_as_error() {
        let run_status = json!({
            "state": "running",
            "exit_code": null,
            "message": "upstream launch started",
            "started_at_epoch_s": 123,
            "runner_pid": u32::MAX,
        });
        let reconciled = reconcile_run_status(Some(run_status), &[]).expect("reconciled");
        assert_eq!(reconciled["state"].as_str(), Some("finished_error"));
        assert_eq!(
            reconciled["message"].as_str(),
            Some("runner pid is no longer alive and result files did not appear")
        );
        assert_eq!(reconciled["result_verdict"].as_str(), Some("no_results"));
        assert_eq!(reconciled["runner_pid"].as_u64(), Some(u32::MAX as u64));
    }

    #[test]
    fn reconcile_run_status_with_runtime_marks_missing_runner_without_pid_as_error() {
        let run_status = json!({
            "state": "running",
            "exit_code": null,
            "message": "upstream launch started",
            "started_at_epoch_s": 123,
        });
        let reconciled = reconcile_run_status_with_runtime(
            Some(run_status),
            &[],
            Some(Path::new("/tmp/amai-nonexistent-run")),
            Some("http://127.0.0.1:19999"),
        )
        .expect("reconciled");
        assert_eq!(reconciled["state"].as_str(), Some("finished_error"));
        assert_eq!(
            reconciled["message"].as_str(),
            Some("runner process is no longer alive and result files did not appear")
        );
        assert_eq!(reconciled["result_verdict"].as_str(), Some("no_results"));
    }

    #[test]
    fn reconcile_run_status_preserves_runner_pid_when_results_already_exist() {
        let run_status = json!({
            "state": "running",
            "exit_code": null,
            "message": "upstream launch started",
            "started_at_epoch_s": 123,
            "runner_pid": 4242,
        });
        let results = vec![ExternalResultSummary {
            path: PathBuf::from("/tmp/result.json"),
            run_id: "run".to_owned(),
            task_label: "task".to_owned(),
            db: "QdrantLocal".to_owned(),
            label: ":)".to_owned(),
            qps: Some(1.0),
            serial_latency_p99: Some(2.0),
            serial_latency_p95: Some(1.5),
            recall: Some(1.0),
            max_load_count: Some(10.0),
            load_duration: Some(20.0),
        }];
        let reconciled = reconcile_run_status(Some(run_status), &results).expect("reconciled");
        assert_eq!(reconciled["state"].as_str(), Some("finished_ok"));
        assert_eq!(reconciled["runner_pid"].as_u64(), Some(4242));
    }

    #[test]
    fn unsupported_dataset_blocks_ann_adapter_honestly() {
        let catalog = sample_catalog();
        let dataset = &catalog.datasets["sphere_10m_meta_dpr"];
        let status = determine_adapter_status(
            "ann_benchmarks",
            dataset,
            true,
            Path::new("/tmp/missing"),
            false,
        );
        assert_eq!(status, AdapterStatus::BlockedUnsupportedDataset);
    }

    #[test]
    fn ann_adapter_blocks_when_upstream_qdrant_is_disabled() {
        let catalog = sample_catalog();
        let dataset = &catalog.datasets["dbpedia_openai_1000k_angular"];
        let temp_root =
            std::env::temp_dir().join(format!("amai-ann-upstream-disabled-{}", std::process::id()));
        let config_dir = temp_root
            .join("ann_benchmarks")
            .join("algorithms")
            .join("qdrant");
        fs::create_dir_all(&config_dir).expect("create qdrant config dir");
        fs::write(config_dir.join("config.yml"), "disabled: true\n").expect("write qdrant config");
        let status = determine_adapter_status("ann_benchmarks", dataset, true, &temp_root, false);
        assert_eq!(status, AdapterStatus::BlockedUpstreamDisabled);
        let _ = fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn ann_dataset_name_comes_from_hdf5_filename() {
        let catalog = sample_catalog();
        let dataset = &catalog.datasets["dbpedia_openai_1000k_angular"];
        assert_eq!(
            ann_benchmark_dataset_name(dataset),
            "dbpedia-openai-1000k-angular"
        );
    }

    fn sample_registry() -> ExternalBenchmarkFile {
        let mut benchmarks = BTreeMap::new();
        benchmarks.insert(
            "vectordbbench".to_owned(),
            ExternalBenchmarkEntry {
                order: 1,
                display_name: "VectorDBBench".to_owned(),
                benchmark_kind: "framework".to_owned(),
                summary: "framework".to_owned(),
                reference_url: "https://example.com/vdb".to_owned(),
                upstream_git_url: "https://example.com/vdb.git".to_owned(),
                aliases: vec!["vector db bench".to_owned()],
                requires_tools: vec!["python3".to_owned(), "docker".to_owned()],
                why_relevant: vec!["why".to_owned()],
                local_role: vec!["role".to_owned()],
                next_step: "next".to_owned(),
            },
        );
        benchmarks.insert(
            "ann_benchmarks".to_owned(),
            ExternalBenchmarkEntry {
                order: 2,
                display_name: "ann-benchmarks".to_owned(),
                benchmark_kind: "ann_core".to_owned(),
                summary: "framework".to_owned(),
                reference_url: "https://example.com/ann".to_owned(),
                upstream_git_url: "https://example.com/ann.git".to_owned(),
                aliases: vec!["ann benchmarks".to_owned()],
                requires_tools: vec!["python3".to_owned(), "docker".to_owned()],
                why_relevant: vec!["why".to_owned()],
                local_role: vec!["role".to_owned()],
                next_step: "next".to_owned(),
            },
        );
        ExternalBenchmarkFile {
            source: ExternalBenchmarkSource {
                display_name: "source".to_owned(),
                summary: "summary".to_owned(),
            },
            benchmarks,
        }
    }

    fn sample_catalog() -> ExternalDatasetFile {
        let mut datasets = BTreeMap::new();
        datasets.insert(
            "dbpedia_openai_1000k_angular".to_owned(),
            ExternalDatasetEntry {
                order: 1,
                display_name: "dbpedia-openai-1000k-angular".to_owned(),
                aliases: vec!["dbpedia".to_owned()],
                family: "hdf5".to_owned(),
                distance: "cosine".to_owned(),
                dimensions: 1536,
                local_filename: "dbpedia-openai-1000k-angular.hdf5".to_owned(),
                download_url: "https://example.com/dbpedia".to_owned(),
                usage_scope: vec!["vectordbbench".to_owned(), "ann_benchmarks".to_owned()],
                why_useful: vec!["why".to_owned()],
            },
        );
        datasets.insert(
            "sift_128_euclidean".to_owned(),
            ExternalDatasetEntry {
                order: 2,
                display_name: "sift-128-euclidean".to_owned(),
                aliases: vec!["sift".to_owned()],
                family: "hdf5".to_owned(),
                distance: "euclidean".to_owned(),
                dimensions: 128,
                local_filename: "sift-128-euclidean.hdf5".to_owned(),
                download_url: "https://example.com/sift".to_owned(),
                usage_scope: vec!["ann_benchmarks".to_owned()],
                why_useful: vec!["why".to_owned()],
            },
        );
        datasets.insert(
            "sphere_10m_meta_dpr".to_owned(),
            ExternalDatasetEntry {
                order: 3,
                display_name: "sphere-10M-meta-dpr".to_owned(),
                aliases: vec!["sphere".to_owned()],
                family: "hdf5".to_owned(),
                distance: "cosine".to_owned(),
                dimensions: 768,
                local_filename: "sphere-10M-meta-dpr.hdf5".to_owned(),
                download_url: "https://example.com/sphere".to_owned(),
                usage_scope: vec!["vectordbbench".to_owned()],
                why_useful: vec!["why".to_owned()],
            },
        );
        ExternalDatasetFile {
            source: ExternalBenchmarkSource {
                display_name: "source".to_owned(),
                summary: "summary".to_owned(),
            },
            storage: ExternalDatasetStorage {
                relative_root: "state/external-benchmarks/datasets".to_owned(),
            },
            datasets,
        }
    }
}
