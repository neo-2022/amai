use anyhow::{Context, Result, anyhow, bail};
use clap::{Args, Parser, Subcommand};
use dirs::{home_dir, state_dir};
use native_tls::TlsConnector as NativeTlsConnector;
use postgres_native_tls::MakeTlsConnector;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio_postgres::config::{Host, SslMode};
use tokio_postgres::{Config as PostgresConfig, NoTls};

#[derive(Debug, Parser)]
#[command(
    name = "memory",
    about = "Amai-backed compatibility bridge for legacy memory commands"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<MemoryCommand>,
}

#[derive(Debug, Subcommand)]
enum MemoryCommand {
    Context(ContextArgs),
    Search(SearchArgs),
    Details(DetailsArgs),
    Save(SaveArgs),
    Mcp(McpArgs),
}

#[derive(Debug, Clone, Args)]
struct ContextArgs {
    #[arg(long, num_args = 0..=1, value_name = "PROJECT")]
    project: Option<Option<String>>,
    #[arg(long)]
    repo_root: Option<PathBuf>,
    #[arg(long, default_value = "continuity")]
    namespace: String,
}

#[derive(Debug, Clone, Args)]
struct SearchArgs {
    #[arg(num_args = 1.., required = true)]
    query: Vec<String>,
    #[arg(long, num_args = 0..=1, value_name = "PROJECT")]
    project: Option<Option<String>>,
    #[arg(long)]
    repo_root: Option<PathBuf>,
    #[arg(long, default_value = "continuity")]
    namespace: String,
    #[arg(long, default_value_t = 5)]
    limit_documents: usize,
    #[arg(long, default_value_t = 8)]
    limit_symbols: usize,
    #[arg(long, default_value_t = 8)]
    limit_chunks: usize,
    #[arg(long, default_value_t = 8)]
    limit_semantic_chunks: usize,
    #[arg(long)]
    client_prompt_tokens: Option<u64>,
    #[arg(long)]
    assistant_generation_tokens: Option<u64>,
    #[arg(long)]
    tool_overhead_tokens: Option<u64>,
    #[arg(long)]
    continuity_restore_tokens: Option<u64>,
}

#[derive(Debug, Clone, Args)]
struct DetailsArgs {
    id: String,
}

#[derive(Debug, Clone, Args)]
struct SaveArgs {
    #[arg(long)]
    title: String,
    #[arg(long)]
    what: Option<String>,
    #[arg(long)]
    why: Option<String>,
    #[arg(long)]
    impact: Option<String>,
    #[arg(long)]
    tags: Option<String>,
    #[arg(long)]
    category: Option<String>,
    #[arg(long)]
    related_files: Option<String>,
    #[arg(long)]
    source: Option<String>,
    #[arg(long)]
    details: Option<String>,
    #[arg(long, num_args = 0..=1, value_name = "PROJECT")]
    project: Option<Option<String>>,
    #[arg(long)]
    repo_root: Option<PathBuf>,
    #[arg(long, default_value = "continuity")]
    namespace: String,
}

#[derive(Debug, Clone, Args)]
struct McpArgs {
    #[arg(trailing_var_arg = true)]
    passthrough: Vec<String>,
}

#[derive(Debug, Clone)]
struct BridgePaths {
    amai_root: PathBuf,
}

#[derive(Debug, Clone)]
struct ResolvedProject {
    project_code: String,
    repo_root: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SearchState {
    query: String,
    project_code: String,
    namespace: String,
    created_at_epoch_ms: u128,
    included_reasons_summary: Option<String>,
    excluded_reasons_summary: Option<String>,
    hits: Vec<SearchHit>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SearchHit {
    id: String,
    kind: String,
    title: String,
    location: String,
    score: Option<f64>,
    summary: String,
    raw: Value,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let paths = BridgePaths::discover()?;

    match cli.command {
        Some(MemoryCommand::Context(args)) => run_context(&paths, &args).await?,
        Some(MemoryCommand::Search(args)) => run_search(&paths, &args).await?,
        Some(MemoryCommand::Details(args)) => run_details(&args)?,
        Some(MemoryCommand::Save(args)) => run_save(&paths, &args).await?,
        Some(MemoryCommand::Mcp(args)) => run_mcp(&paths, &args)?,
        None => print_bridge_help(),
    }

    Ok(())
}

impl BridgePaths {
    fn discover() -> Result<Self> {
        if let Ok(value) = env::var("AMAI_REPO_ROOT") {
            let path = PathBuf::from(value);
            if is_amai_root(&path) {
                return Ok(Self {
                    amai_root: canonical(&path)?,
                });
            }
        }

        if let Ok(current_exe) = env::current_exe()
            && let Some(root) = search_amai_root_from(&current_exe)
        {
            return Ok(Self { amai_root: root });
        }

        if let Some(root) = load_amai_root_from_codex_config()? {
            return Ok(Self { amai_root: root });
        }

        for candidate in conventional_amai_roots() {
            if is_amai_root(&candidate) {
                return Ok(Self {
                    amai_root: canonical(&candidate)?,
                });
            }
        }

        bail!(
            "failed to discover Amai repo root; set AMAI_REPO_ROOT or install Amai in a conventional location"
        )
    }

    fn amai_command(&self) -> Result<Command> {
        if let Ok(path) = env::var("AMAI_BRIDGE_BINARY") {
            let mut command = Command::new(path);
            command.current_dir(&self.amai_root);
            return Ok(command);
        }
        let release = compiled_binary_path(&self.amai_root, "target/release", "amai");
        if release.is_file() {
            let mut command = Command::new(release);
            command.current_dir(&self.amai_root);
            return Ok(command);
        }
        let debug = compiled_binary_path(&self.amai_root, "target/debug", "amai");
        if debug.is_file() {
            let mut command = Command::new(debug);
            command.current_dir(&self.amai_root);
            return Ok(command);
        }
        bail!(
            "failed to find built Amai binary in {}; run cargo build --release or set AMAI_BRIDGE_BINARY explicitly",
            self.amai_root.display()
        )
    }
}

fn compiled_binary_path(repo_root: &Path, directory: &str, stem: &str) -> PathBuf {
    let suffix = std::env::consts::EXE_SUFFIX;
    repo_root.join(directory).join(format!("{stem}{suffix}"))
}

async fn run_context(paths: &BridgePaths, args: &ContextArgs) -> Result<()> {
    let resolved = resolve_project(paths, args.project.clone(), args.repo_root.clone()).await?;
    let mut command = paths.amai_command()?;
    apply_default_agent_scope(
        &mut command,
        &resolved.project_code,
        &args.namespace,
        "startup",
    );
    command.arg("continuity").arg("startup");
    if let Some(repo_root) = resolved.repo_root {
        command.arg("--repo-root").arg(repo_root);
    } else {
        command.arg("--project").arg(resolved.project_code);
    }
    command.arg("--namespace").arg(&args.namespace);
    run_inheriting(command).context("Amai continuity startup failed")
}

async fn run_search(paths: &BridgePaths, args: &SearchArgs) -> Result<()> {
    let resolved = resolve_project(paths, args.project.clone(), args.repo_root.clone()).await?;
    let query = args.query.join(" ");
    let mut command = paths.amai_command()?;
    apply_default_agent_scope(
        &mut command,
        &resolved.project_code,
        &args.namespace,
        "search",
    );
    command
        .arg("context")
        .arg("pack")
        .arg("--project")
        .arg(&resolved.project_code)
        .arg("--namespace")
        .arg(&args.namespace)
        .arg("--query")
        .arg(&query)
        .arg("--limit-documents")
        .arg(args.limit_documents.to_string())
        .arg("--limit-symbols")
        .arg(args.limit_symbols.to_string())
        .arg("--limit-chunks")
        .arg(args.limit_chunks.to_string())
        .arg("--limit-semantic-chunks")
        .arg(args.limit_semantic_chunks.to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    append_whole_cycle_observed_args(
        &mut command,
        args.client_prompt_tokens,
        args.assistant_generation_tokens,
        args.tool_overhead_tokens,
        args.continuity_restore_tokens,
    );
    let output = command
        .output()
        .with_context(|| "failed to run Amai context pack")?;
    if !output.status.success() {
        bail!("Amai context pack failed");
    }
    let payload: Value =
        serde_json::from_slice(&output.stdout).context("Amai context pack did not return JSON")?;
    let hits = build_search_hits(&payload);
    let state = SearchState {
        query,
        project_code: resolved.project_code,
        namespace: args.namespace.clone(),
        created_at_epoch_ms: now_epoch_ms(),
        included_reasons_summary: decision_trace_summary(&payload["decision_trace"], "included"),
        excluded_reasons_summary: decision_trace_summary(
            &payload["decision_trace"],
            "not_included",
        ),
        hits,
    };
    save_search_state(&state)?;
    print_search_state(&state);
    Ok(())
}

fn run_details(args: &DetailsArgs) -> Result<()> {
    let state = load_search_state()?;
    let hit = if let Ok(index) = args.id.parse::<usize>() {
        state
            .hits
            .get(index.saturating_sub(1))
            .ok_or_else(|| anyhow!("search hit {} not found", args.id))?
    } else {
        state
            .hits
            .iter()
            .find(|hit| hit.id == args.id)
            .ok_or_else(|| anyhow!("search hit {} not found", args.id))?
    };

    println!("Amai memory details");
    println!();
    println!("ID: {}", hit.id);
    println!("Тип: {}", hit.kind);
    println!("Заголовок: {}", hit.title);
    println!("Где: {}", hit.location);
    if let Some(score) = hit.score {
        println!("Score: {:.3}", score);
    }
    println!("Кратко: {}", hit.summary);
    println!();
    println!("{}", serde_json::to_string_pretty(&hit.raw)?);
    Ok(())
}

async fn run_save(paths: &BridgePaths, args: &SaveArgs) -> Result<()> {
    let resolved = resolve_project(paths, args.project.clone(), args.repo_root.clone()).await?;
    let next_step = args
        .impact
        .as_deref()
        .or(args.why.as_deref())
        .or(args.what.as_deref())
        .unwrap_or("Продолжить следующую рабочую линию из этого решения.")
        .to_string();
    let details = render_save_details(args);
    let details_path = env::temp_dir().join(format!(
        "amai-memory-save-{}-{}.md",
        std::process::id(),
        now_epoch_ms()
    ));
    fs::write(&details_path, details)
        .with_context(|| format!("failed to write {}", details_path.display()))?;

    let mut command = paths.amai_command()?;
    apply_default_agent_scope(
        &mut command,
        &resolved.project_code,
        &args.namespace,
        "handoff",
    );
    command
        .arg("continuity")
        .arg("handoff")
        .arg("--project")
        .arg(&resolved.project_code)
        .arg("--namespace")
        .arg(&args.namespace)
        .arg("--headline")
        .arg(&args.title)
        .arg("--next-step")
        .arg(&next_step)
        .arg("--details-file")
        .arg(&details_path);
    let result = run_inheriting(command).context("Amai continuity handoff failed");
    let _ = fs::remove_file(&details_path);
    result
}

fn run_mcp(paths: &BridgePaths, args: &McpArgs) -> Result<()> {
    let mut command = paths.amai_command()?;
    command.arg("mcp").arg("serve");
    for arg in &args.passthrough {
        if arg != "serve" {
            command.arg(arg);
        }
    }
    run_inheriting(command).context("Amai MCP bridge failed")
}

fn apply_default_agent_scope(
    command: &mut Command,
    project_code: &str,
    namespace: &str,
    suffix: &str,
) {
    if env::var("AMAI_AGENT_SCOPE")
        .ok()
        .as_deref()
        .unwrap_or("")
        .is_empty()
    {
        command.env(
            "AMAI_AGENT_SCOPE",
            format!("{project_code}::{namespace}::memory-bridge::{suffix}"),
        );
    }
}

fn append_whole_cycle_observed_args(
    command: &mut Command,
    client_prompt_tokens: Option<u64>,
    assistant_generation_tokens: Option<u64>,
    tool_overhead_tokens: Option<u64>,
    continuity_restore_tokens: Option<u64>,
) {
    if let Some(tokens) = client_prompt_tokens {
        command
            .arg("--client-prompt-tokens")
            .arg(tokens.to_string());
    }
    if let Some(tokens) = assistant_generation_tokens {
        command
            .arg("--assistant-generation-tokens")
            .arg(tokens.to_string());
    }
    if let Some(tokens) = tool_overhead_tokens {
        command
            .arg("--tool-overhead-tokens")
            .arg(tokens.to_string());
    }
    if let Some(tokens) = continuity_restore_tokens {
        command
            .arg("--continuity-restore-tokens")
            .arg(tokens.to_string());
    }
}

async fn resolve_project(
    paths: &BridgePaths,
    project_flag: Option<Option<String>>,
    repo_root_flag: Option<PathBuf>,
) -> Result<ResolvedProject> {
    if let Some(Some(project_code)) = project_flag {
        return Ok(ResolvedProject {
            project_code,
            repo_root: None,
        });
    }

    let working_dir = match repo_root_flag {
        Some(path) => canonical(&path)?,
        None => canonical(&env::current_dir().context("failed to read current directory")?)?,
    };

    let project_code = lookup_project_code(paths, &working_dir).await?;
    Ok(ResolvedProject {
        project_code,
        repo_root: Some(working_dir),
    })
}

fn match_project_code_by_repo_roots<'a, I>(working_dir: &Path, candidates: I) -> Option<String>
where
    I: IntoIterator<Item = (&'a str, &'a str)>,
{
    candidates
        .into_iter()
        .filter_map(|(code, repo_root)| {
            let candidate = Path::new(repo_root);
            working_dir
                .starts_with(candidate)
                .then(|| (candidate.as_os_str().len(), code.to_string()))
        })
        .max_by_key(|(length, _)| *length)
        .map(|(_, code)| code)
}

async fn lookup_project_code(paths: &BridgePaths, working_dir: &Path) -> Result<String> {
    dotenvy::from_path_override(paths.amai_root.join(".env")).ok();
    let dsn = env::var("AMI_POSTGRES_DSN").context("missing AMI_POSTGRES_DSN")?;
    let config: PostgresConfig = dsn
        .parse()
        .with_context(|| format!("invalid postgres dsn {}", safe_postgres_descriptor(&dsn)))?;
    let masked_descriptor = safe_postgres_descriptor_from_config(&config);
    let ssl_mode = config.get_ssl_mode();
    let client = match ssl_mode {
        SslMode::Disable => {
            let (client, connection) = config.connect(NoTls).await.with_context(|| {
                format!("failed to connect to postgres via {masked_descriptor}")
            })?;
            tokio::spawn(async move {
                let _ = connection.await;
            });
            client
        }
        _ => {
            let connector = NativeTlsConnector::builder()
                .build()
                .context("failed to build native TLS connector")?;
            let connector = MakeTlsConnector::new(connector);
            let (client, connection) = config.connect(connector).await.with_context(|| {
                format!("failed to connect to postgres via {masked_descriptor}")
            })?;
            tokio::spawn(async move {
                let _ = connection.await;
            });
            client
        }
    };
    let rows = client
        .query(
            r#"
            SELECT p.code, r.repo_root
            FROM ami.project_repo_roots r
            INNER JOIN ami.projects p ON p.project_id = r.project_id
            ORDER BY length(r.repo_root) DESC, p.code ASC
            "#,
            &[],
        )
        .await
        .context("failed to list Amai projects")?;
    let candidates: Vec<(String, String)> = rows
        .into_iter()
        .map(|row| (row.get(0), row.get(1)))
        .collect();
    if let Some(project_code) = match_project_code_by_repo_roots(
        working_dir,
        candidates
            .iter()
            .map(|(code, repo_root)| (code.as_str(), repo_root.as_str())),
    ) {
        return Ok(project_code);
    }
    bail!(
        "failed to resolve project from {}; pass --project <code> explicitly",
        working_dir.display()
    )
}

fn safe_postgres_descriptor(dsn: &str) -> String {
    dsn.parse::<PostgresConfig>()
        .map(|config| safe_postgres_descriptor_from_config(&config))
        .unwrap_or_else(|_| "postgres://[redacted-invalid-dsn]".to_string())
}

fn safe_postgres_descriptor_from_config(config: &PostgresConfig) -> String {
    let user = config.get_user().unwrap_or("unknown");
    let dbname = config.get_dbname().unwrap_or("postgres");
    let ssl_mode = match config.get_ssl_mode() {
        SslMode::Disable => "disable",
        SslMode::Prefer => "prefer",
        SslMode::Require => "require",
        _ => "unknown",
    };
    let host = config
        .get_hosts()
        .first()
        .map(postgres_host_label)
        .unwrap_or_else(|| "localhost".to_string());
    let port = config.get_ports().first().copied().unwrap_or(5432);
    format!(
        "postgres://{}:***@{}:{}/{}?sslmode={}",
        user, host, port, dbname, ssl_mode
    )
}

fn postgres_host_label(host: &Host) -> String {
    match host {
        Host::Tcp(host) => host.clone(),
        #[cfg(unix)]
        Host::Unix(path) => format!("unix:{}", path.display()),
    }
}

fn build_search_hits(payload: &Value) -> Vec<SearchHit> {
    let retrieval = &payload["retrieval"];
    let mut hits = Vec::new();
    append_document_hits(&mut hits, &retrieval["exact_documents"]);
    append_symbol_hits(&mut hits, &retrieval["symbol_hits"]);
    append_chunk_hits(&mut hits, "lexical_chunk", &retrieval["lexical_chunks"]);
    append_chunk_hits(&mut hits, "semantic_chunk", &retrieval["semantic_chunks"]);
    let filtered = hits
        .iter()
        .filter(|hit| !contains_legacy_bridge_marker(hit))
        .cloned()
        .collect::<Vec<_>>();
    if !filtered.is_empty() {
        hits = filtered;
    }
    hits.truncate(8);
    for (index, hit) in hits.iter_mut().enumerate() {
        hit.id = (index + 1).to_string();
    }
    hits
}

fn append_document_hits(target: &mut Vec<SearchHit>, value: &Value) {
    let Some(items) = value.as_array() else {
        return;
    };
    for item in items {
        target.push(SearchHit {
            id: String::new(),
            kind: "document".to_string(),
            title: item["relative_path"]
                .as_str()
                .unwrap_or("document")
                .to_string(),
            location: format!(
                "{} :: {}",
                item["project_code"].as_str().unwrap_or("?"),
                item["relative_path"].as_str().unwrap_or("?")
            ),
            score: item["score"].as_f64(),
            summary: summarize_text(item["snippet"].as_str().unwrap_or("snippet not available")),
            raw: item.clone(),
        });
    }
}

fn append_symbol_hits(target: &mut Vec<SearchHit>, value: &Value) {
    let Some(items) = value.as_array() else {
        return;
    };
    for item in items {
        let name = item["name"].as_str().unwrap_or("symbol");
        let relative_path = item["relative_path"].as_str().unwrap_or("?");
        let start_line = item["start_line"].as_i64().unwrap_or_default();
        target.push(SearchHit {
            id: String::new(),
            kind: "symbol".to_string(),
            title: format!("{} :: {}", name, item["kind"].as_str().unwrap_or("symbol")),
            location: format!(
                "{} :: {}:{}",
                item["project_code"].as_str().unwrap_or("?"),
                relative_path,
                start_line
            ),
            score: item["score"].as_f64(),
            summary: summarize_text(
                item["metadata"]["signature"]
                    .as_str()
                    .or_else(|| item["metadata"]["detail"].as_str())
                    .unwrap_or(relative_path),
            ),
            raw: item.clone(),
        });
    }
}

fn append_chunk_hits(target: &mut Vec<SearchHit>, kind: &str, value: &Value) {
    let Some(items) = value.as_array() else {
        return;
    };
    for item in items {
        let relative_path = item["relative_path"].as_str().unwrap_or("?");
        let start_line = item["start_line"].as_i64().unwrap_or_default();
        target.push(SearchHit {
            id: String::new(),
            kind: kind.to_string(),
            title: relative_path.to_string(),
            location: format!(
                "{} :: {}:{}",
                item["project_code"].as_str().unwrap_or("?"),
                relative_path,
                start_line
            ),
            score: item["score"].as_f64(),
            summary: summarize_text(item["content"].as_str().unwrap_or("content not available")),
            raw: item.clone(),
        });
    }
}

fn summarize_text(input: &str) -> String {
    let collapsed = input.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut preview = collapsed.chars().take(260).collect::<String>();
    if collapsed.chars().count() > 260 {
        preview.push_str("...");
    }
    preview
}

fn strategy_label(strategy: &str) -> &str {
    match strategy {
        "exact_documents" => "точные совпадения",
        "symbol_hits" => "совпадения по символам",
        "lexical_chunks" => "текстовые фрагменты",
        "semantic_chunks" => "смысловые фрагменты",
        _ => strategy,
    }
}

fn decision_trace_summary(trace: &Value, key: &str) -> Option<String> {
    let items = trace.get(key)?.as_array()?;
    let parts = items
        .iter()
        .take(3)
        .filter_map(|item| {
            let reason = item["reason"].as_str()?.trim();
            if reason.is_empty() {
                return None;
            }
            let strategy = strategy_label(item["strategy"].as_str().unwrap_or_default());
            let count = item["count"].as_u64();
            Some(match count {
                Some(value) if value > 0 => format!("{strategy} ({value}) — {reason}"),
                _ => format!("{strategy} — {reason}"),
            })
        })
        .collect::<Vec<_>>();
    if parts.is_empty() {
        None
    } else {
        Some(summarize_text(&parts.join(" • ")))
    }
}

fn contains_legacy_bridge_marker(hit: &SearchHit) -> bool {
    let lowered = format!("{} {}", hit.title, hit.summary).to_lowercase();
    lowered.contains("echovault")
        || lowered.contains("memory_context")
        || lowered.contains("echovault-project-bootstrap")
}

fn print_search_state(state: &SearchState) {
    println!("Amai memory search");
    println!();
    println!("Проект: {}", state.project_code);
    println!("Namespace: {}", state.namespace);
    println!("Запрос: {}", state.query);
    if let Some(summary) = &state.included_reasons_summary {
        println!("Почему вошло: {summary}");
    }
    if let Some(summary) = &state.excluded_reasons_summary {
        println!("Почему часть не вошла: {summary}");
    }
    println!("Найдено записей: {}", state.hits.len());
    println!();
    for hit in &state.hits {
        println!("[{}] {} :: {}", hit.id, hit.kind, hit.title);
        println!("     Где: {}", hit.location);
        if let Some(score) = hit.score {
            println!("     Score: {:.3}", score);
        }
        println!("     {}", hit.summary);
    }
    if !state.hits.is_empty() {
        println!();
        println!("Чтобы открыть одну запись подробнее: memory details <номер>");
    }
}

fn render_save_details(args: &SaveArgs) -> String {
    let mut lines = Vec::new();
    lines.push(format!("# {}", args.title));
    if let Some(value) = &args.what {
        lines.push(String::new());
        lines.push("## Что".to_string());
        lines.push(value.clone());
    }
    if let Some(value) = &args.why {
        lines.push(String::new());
        lines.push("## Почему".to_string());
        lines.push(value.clone());
    }
    if let Some(value) = &args.impact {
        lines.push(String::new());
        lines.push("## Влияние".to_string());
        lines.push(value.clone());
    }
    if let Some(value) = &args.category {
        lines.push(String::new());
        lines.push(format!("Категория: {value}"));
    }
    if let Some(value) = &args.tags {
        lines.push(format!("Теги: {value}"));
    }
    if let Some(value) = &args.related_files {
        lines.push(format!("Связанные файлы: {value}"));
    }
    if let Some(value) = &args.source {
        lines.push(format!("Источник: {value}"));
    }
    if let Some(value) = &args.details {
        lines.push(String::new());
        lines.push("## Детали".to_string());
        lines.push(value.clone());
    }
    lines.join("\n")
}

fn save_search_state(state: &SearchState) -> Result<()> {
    let path = search_state_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let content = serde_json::to_string_pretty(state)?;
    fs::write(&path, content).with_context(|| format!("failed to write {}", path.display()))
}

fn load_search_state() -> Result<SearchState> {
    let path = search_state_path()?;
    let content =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_json::from_str(&content).context("failed to parse saved memory search state")
}

fn search_state_path() -> Result<PathBuf> {
    if let Some(base) = state_dir().or_else(|| home_dir().map(|home| home.join(".local/state"))) {
        return Ok(base.join("amai/memory_search_state.json"));
    }
    bail!("failed to resolve state directory for memory search state")
}

fn run_inheriting(mut command: Command) -> Result<()> {
    let status = command
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .context("failed to start command")?;
    ensure_success(status)
}

fn ensure_success(status: ExitStatus) -> Result<()> {
    if status.success() {
        return Ok(());
    }
    bail!("command exited with status {}", status)
}

fn canonical(path: &Path) -> Result<PathBuf> {
    path.canonicalize()
        .with_context(|| format!("failed to resolve {}", path.display()))
}

fn is_amai_root(path: &Path) -> bool {
    path.join("Cargo.toml").is_file()
        && path.join("compose.yaml").is_file()
        && path.join("scripts/run_mcp_stdio.sh").is_file()
}

fn search_amai_root_from(path: &Path) -> Option<PathBuf> {
    for ancestor in path.ancestors() {
        if is_amai_root(ancestor) {
            return canonical(ancestor).ok();
        }
    }
    None
}

fn conventional_amai_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(home) = home_dir() {
        roots.push(home.join("agent-memory-index"));
        roots.push(home.join(".codex/tools/agent-memory-index"));
    }
    roots
}

fn load_amai_root_from_codex_config() -> Result<Option<PathBuf>> {
    let Some(home) = home_dir() else {
        return Ok(None);
    };
    let path = home.join(".codex/config.toml");
    if !path.is_file() {
        return Ok(None);
    }
    let content =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    let value: toml::Value = toml::from_str(&content).context("failed to parse codex config")?;
    let Some(command) = value
        .get("mcp_servers")
        .and_then(|root| root.get("amai"))
        .and_then(|amai| amai.get("command"))
        .and_then(toml::Value::as_str)
    else {
        return Ok(None);
    };
    let command_path = PathBuf::from(command);
    if command_path.ends_with("scripts/run_mcp_stdio.sh")
        || command_path.ends_with("scripts/run_mcp_stdio.cmd")
        || command_path.ends_with("scripts/run_mcp_stdio.ps1")
    {
        let Some(parent) = command_path.parent().and_then(Path::parent) else {
            return Ok(None);
        };
        if is_amai_root(parent) {
            return Ok(Some(canonical(parent)?));
        }
    }
    Ok(None)
}

fn print_bridge_help() {
    println!("Amai memory bridge");
    println!();
    println!("Поддерживаемые команды:");
    println!("- memory context [--project [code]] [--repo-root PATH]");
    println!("- memory search <запрос> [--project [code]] [--repo-root PATH]");
    println!(
        "  memory search также принимает optional whole-cycle flags: --client-prompt-tokens, --assistant-generation-tokens, --tool-overhead-tokens, --continuity-restore-tokens"
    );
    println!("- memory details <номер>");
    println!("- memory save --title ... [--what ... --why ... --impact ...]");
    println!("- memory mcp");
}

fn now_epoch_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_millis())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{
        BridgePaths, Cli, append_whole_cycle_observed_args, build_search_hits,
        decision_trace_summary, is_amai_root, match_project_code_by_repo_roots,
        render_save_details,
    };
    use clap::Parser;
    use serde_json::json;
    use std::fs;
    use std::path::Path;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn renders_save_details_human_readably() {
        let args = super::SaveArgs {
            title: "Decision".to_string(),
            what: Some("Did something".to_string()),
            why: Some("It mattered".to_string()),
            impact: Some("Changed behavior".to_string()),
            tags: Some("amai,bridge".to_string()),
            category: Some("decision".to_string()),
            related_files: Some("src/bin/memory.rs".to_string()),
            source: Some("codex".to_string()),
            details: Some("Longer details".to_string()),
            project: None,
            repo_root: None,
            namespace: "continuity".to_string(),
        };
        let rendered = render_save_details(&args);
        assert!(rendered.contains("# Decision"));
        assert!(rendered.contains("## Что"));
        assert!(rendered.contains("Теги: amai,bridge"));
    }

    #[test]
    fn builds_hits_from_context_pack_payload() {
        let payload = json!({
            "retrieval": {
                "exact_documents": [{
                    "project_code": "art",
                    "relative_path": "README.md",
                    "score": 0.9,
                    "snippet": "hello"
                }],
                "symbol_hits": [{
                    "project_code": "art",
                    "relative_path": "src/lib.rs",
                    "name": "run",
                    "kind": "function",
                    "score": 0.8,
                    "start_line": 12,
                    "metadata": { "signature": "fn run()" }
                }],
                "lexical_chunks": [{
                    "project_code": "art",
                    "relative_path": "docs/a.md",
                    "start_line": 4,
                    "score": 0.7,
                    "content": "chunk"
                }],
                "semantic_chunks": []
            }
        });
        let hits = build_search_hits(&payload);
        assert_eq!(hits.len(), 3);
        assert_eq!(hits[0].id, "1");
        assert_eq!(hits[1].kind, "symbol");
    }

    #[test]
    fn decision_trace_summary_compacts_included_and_missing_layers() {
        let trace = json!({
            "included": [{
                "strategy": "exact_documents",
                "count": 1,
                "reason": "Нашлись точные совпадения по continuity."
            }],
            "not_included": [{
                "strategy": "semantic_chunks",
                "reason": "Semantic layer честно abstained и не добавил фрагменты."
            }]
        });
        assert_eq!(
            decision_trace_summary(&trace, "included").as_deref(),
            Some("точные совпадения (1) — Нашлись точные совпадения по continuity.")
        );
        assert_eq!(
            decision_trace_summary(&trace, "not_included").as_deref(),
            Some("смысловые фрагменты — Semantic layer честно abstained и не добавил фрагменты.")
        );
    }

    #[test]
    fn root_check_requires_expected_markers() {
        assert!(!is_amai_root(Path::new("/tmp")));
    }

    #[test]
    fn amai_command_requires_built_binary_or_override() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let amai_root = std::env::temp_dir().join(format!("amai-bridge-missing-{unique}"));
        fs::create_dir_all(&amai_root).expect("temp root");
        let paths = BridgePaths {
            amai_root: amai_root.clone(),
        };
        let error = paths.amai_command().expect_err("missing binary must fail");
        assert!(
            error
                .to_string()
                .contains("failed to find built Amai binary")
        );
        let _ = fs::remove_dir_all(&amai_root);
    }

    #[test]
    fn amai_command_prefers_release_binary_when_present() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let amai_root = std::env::temp_dir().join(format!("amai-bridge-release-{unique}"));
        let release_dir = amai_root.join("target/release");
        fs::create_dir_all(&release_dir).expect("release dir");
        let release_binary = super::compiled_binary_path(&amai_root, "target/release", "amai");
        fs::write(&release_binary, b"").expect("release binary placeholder");
        let paths = BridgePaths {
            amai_root: amai_root.clone(),
        };
        let command = paths.amai_command().expect("release command");
        assert_eq!(command.get_program(), release_binary.as_os_str());
        let _ = fs::remove_dir_all(&amai_root);
    }

    #[test]
    fn match_project_code_by_repo_roots_prefers_longest_matching_root() {
        let working_dir = Path::new("/tmp/amai/project-a/nested/worktree");
        let candidates = [
            ("project_parent", "/tmp/amai"),
            ("project_a", "/tmp/amai/project-a"),
            ("project_a_nested", "/tmp/amai/project-a/nested"),
        ];
        let resolved = match_project_code_by_repo_roots(working_dir, candidates);
        assert_eq!(resolved.as_deref(), Some("project_a_nested"));
    }

    #[test]
    fn match_project_code_by_repo_roots_returns_none_when_path_is_unbound() {
        let working_dir = Path::new("/tmp/amai/foreign-project");
        let candidates = [("project_a", "/tmp/amai/project-a")];
        assert!(match_project_code_by_repo_roots(working_dir, candidates).is_none());
    }

    #[test]
    fn search_cli_accepts_whole_cycle_observed_flags() {
        let cli = Cli::parse_from([
            "memory",
            "search",
            "same meter",
            "--client-prompt-tokens",
            "42",
            "--assistant-generation-tokens",
            "24",
            "--tool-overhead-tokens",
            "7",
            "--continuity-restore-tokens",
            "3",
        ]);

        let super::Cli {
            command: Some(super::MemoryCommand::Search(args)),
        } = cli
        else {
            panic!("expected search command");
        };

        assert_eq!(args.client_prompt_tokens, Some(42));
        assert_eq!(args.assistant_generation_tokens, Some(24));
        assert_eq!(args.tool_overhead_tokens, Some(7));
        assert_eq!(args.continuity_restore_tokens, Some(3));
    }

    #[test]
    fn append_whole_cycle_observed_args_adds_expected_flags() {
        let mut command = std::process::Command::new("echo");
        append_whole_cycle_observed_args(&mut command, Some(42), Some(24), Some(7), Some(3));
        let args: Vec<_> = command
            .get_args()
            .map(|item| item.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            args,
            vec![
                "--client-prompt-tokens",
                "42",
                "--assistant-generation-tokens",
                "24",
                "--tool-overhead-tokens",
                "7",
                "--continuity-restore-tokens",
                "3",
            ]
        );
    }
}
