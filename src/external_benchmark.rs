use anyhow::{Context, Result, anyhow};
use reqwest::Client;
use serde::Deserialize;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::fs as tokio_fs;
use tokio::io::AsyncWriteExt;

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
        println!("Полезная стартовая команда для HDF5-style контура:");
        println!(
            "- DATASET={}/dbpedia-openai-1000k-angular.hdf5 DISTANCE=cosine docker compose up --abort-on-container-exit",
            dataset_root.display()
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

    let adapter_kind = if benchmark_code == "vectordbbench" {
        "conversion_required"
    } else {
        "direct_hdf5"
    };
    let status = if adapter_kind == "conversion_required" {
        AdapterStatus::BlockedConversionRequired
    } else if !dataset_path.exists() {
        AdapterStatus::BlockedDatasetMissing
    } else {
        AdapterStatus::Prepared
    };
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
    let launch_commands =
        build_launch_commands(benchmark_code, &upstream_dir, &dataset_path, dataset);
    let comparison_commands = vec![
        "cargo run -- verify cold-path --manifest config/cold_benchmark_manifest.toml".to_owned(),
        "./scripts/proof_load.sh".to_owned(),
        "./scripts/proof_accuracy.sh".to_owned(),
    ];
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
        "upstream_repo_url": benchmark.upstream_git_url,
        "upstream_clone_dir": upstream_dir,
        "launch_commands": launch_commands,
        "comparison_commands": comparison_commands,
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
    if status == AdapterStatus::BlockedDatasetMissing {
        println!("Причина: dataset пока не скачан. Можно повторить с `--download-missing`.");
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

fn build_launch_commands(
    benchmark_code: &str,
    upstream_dir: &Path,
    dataset_path: &Path,
    dataset: &ExternalDatasetEntry,
) -> Vec<String> {
    match benchmark_code {
        "ann_benchmarks" => vec![
            format!(
                "git clone https://github.com/erikbern/ann-benchmarks.git {}",
                upstream_dir.display()
            ),
            format!("cd {}", upstream_dir.display()),
            format!(
                "DATASET={} DISTANCE={} docker compose up --abort-on-container-exit",
                dataset_path.display(),
                dataset.distance
            ),
        ],
        "vectordbbench" => vec![
            format!(
                "git clone https://github.com/zilliztech/VectorDBBench.git {}",
                upstream_dir.display()
            ),
            format!("cd {}", upstream_dir.display()),
            format!(
                "# Сначала преобразовать {} в Parquet bundle: train.parquet / test.parquet / neighbors.parquet",
                dataset_path.display()
            ),
            "docker compose up".to_owned(),
        ],
        _ => vec![format!(
            "# launch contract not defined for {}",
            benchmark_code
        )],
    }
}

fn adapter_status_code(status: AdapterStatus) -> &'static str {
    match status {
        AdapterStatus::Prepared => "prepared",
        AdapterStatus::BlockedConversionRequired => "blocked_conversion_required",
        AdapterStatus::BlockedDatasetMissing => "blocked_dataset_missing",
    }
}

fn adapter_status_label(status: AdapterStatus) -> &'static str {
    match status {
        AdapterStatus::Prepared => "готов к следующему запуску",
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
        ctx.launch_commands
            .iter()
            .map(|line| format!("echo \"{}\"\n", shell_escape_echo(line)))
            .collect::<String>()
    } else if ctx.status == AdapterStatus::BlockedDatasetMissing {
        format!(
            "echo \"Dataset ещё не скачан: {}\"\nexit 2\n",
            ctx.dataset_path.display()
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
        ExternalBenchmarkEntry, ExternalBenchmarkFile, ExternalBenchmarkSource,
        ExternalDatasetEntry, ExternalDatasetFile, ExternalDatasetStorage, normalize_key,
        ordered_benchmarks, recommended_datasets, resolve_benchmark, resolve_dataset,
    };
    use std::collections::BTreeMap;

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
