use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "amai")]
#[command(bin_name = "amai")]
#[command(
    about = "Art-memory-agent-index (Amai): Rust-first control plane for multi-project AI-agent continuity and retrieval"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Benchmark {
        #[command(subcommand)]
        command: BenchmarkCommand,
    },
    Continuity {
        #[command(subcommand)]
        command: ContinuityCommand,
    },
    Deployment {
        #[command(subcommand)]
        command: DeploymentCommand,
    },
    Bootstrap {
        #[command(subcommand)]
        command: BootstrapCommand,
    },
    Compat {
        #[command(subcommand)]
        command: CompatCommand,
    },
    Status,
    Project {
        #[command(subcommand)]
        command: ProjectCommand,
    },
    Namespace {
        #[command(subcommand)]
        command: NamespaceCommand,
    },
    Relation {
        #[command(subcommand)]
        command: RelationCommand,
    },
    Context {
        #[command(subcommand)]
        command: ContextCommand,
    },
    Index {
        #[command(subcommand)]
        command: IndexCommand,
    },
    Verify {
        #[command(subcommand)]
        command: VerifyCommand,
    },
    Observe {
        #[command(subcommand)]
        command: ObserveCommand,
    },
    Mcp {
        #[command(subcommand)]
        command: McpCommand,
    },
}

#[derive(Debug, Subcommand)]
pub enum BenchmarkCommand {
    List,
    Coverage,
    Explain(BenchmarkArgs),
}

#[derive(Debug, Subcommand)]
pub enum DeploymentCommand {
    List,
    Explain(DeploymentTargetArgs),
    Preflight(DeploymentTargetArgs),
}

#[derive(Debug, Subcommand)]
pub enum ContinuityCommand {
    Import(ContinuityImportArgs),
    EnrichThreadIndex(ContinuityThreadIndexEnrichArgs),
    Startup(ContinuityStartupArgs),
    Restore(ContinuityStartupArgs),
    Answer(ContinuityAnswerArgs),
    Handoff(ContinuityHandoffArgs),
}

#[derive(Debug, Subcommand)]
pub enum BootstrapCommand {
    Stack(BootstrapStackArgs),
    Preflight(BootstrapPreflightArgs),
    Install(BootstrapOnboardingArgs),
    Onboarding(BootstrapOnboardingArgs),
    Remove(BootstrapDisconnectArgs),
    Disconnect(BootstrapDisconnectArgs),
}

#[derive(Debug, Subcommand)]
pub enum CompatCommand {
    Check,
}

#[derive(Debug, Subcommand)]
pub enum ProjectCommand {
    Register(ProjectRegisterArgs),
    List,
}

#[derive(Debug, Subcommand)]
pub enum NamespaceCommand {
    Ensure(NamespaceEnsureArgs),
}

#[derive(Debug, Subcommand)]
pub enum RelationCommand {
    Add(RelationAddArgs),
}

#[derive(Debug, Subcommand)]
pub enum ContextCommand {
    Pack(ContextPackArgs),
    Warm(WarmupCacheArgs),
}

#[derive(Debug, Subcommand)]
pub enum IndexCommand {
    Project(IndexProjectArgs),
}

#[derive(Debug, Subcommand)]
pub enum VerifyCommand {
    Benchmark(Box<VerifyBenchmarkArgs>),
    TokenBenchmark(Box<VerifyTokenBenchmarkArgs>),
    TokenBenchmarkSuite(Box<VerifyTokenBenchmarkSuiteArgs>),
    TextCompare(Box<VerifyTextCompareArgs>),
    Accuracy(VerifyAccuracyArgs),
    Load(Box<VerifyLoadArgs>),
    Hostile(VerifyHostileArgs),
    Mcp(Box<VerifyMcpArgs>),
}

#[derive(Debug, Subcommand)]
pub enum ObserveCommand {
    Snapshot,
    SlaCheck,
    TokenReport(ObserveTokenReportArgs),
    RepairTokenLedger(ObserveRepairTokenLedgerArgs),
    ReverifyTokenLedger(ObserveReverifyTokenLedgerArgs),
    Serve(ObserveServeArgs),
}

#[derive(Debug, Subcommand)]
pub enum McpCommand {
    Serve,
    Config(McpConfigArgs),
}

#[derive(Debug, Clone, Args)]
pub struct BenchmarkArgs {
    #[arg(long)]
    pub benchmark: String,
}

#[derive(Debug, Clone, Args)]
pub struct DeploymentTargetArgs {
    #[arg(long)]
    pub target: String,
}

#[derive(Debug, Clone, Args)]
pub struct ContinuityImportArgs {
    #[arg(long)]
    pub project: String,
    #[arg(long)]
    pub display_name: String,
    #[arg(long)]
    pub repo_root: PathBuf,
    #[arg(long, default_value = "continuity")]
    pub namespace: String,
    #[arg(long)]
    pub bootstrap_file: PathBuf,
    #[arg(long)]
    pub thread_index_file: Option<PathBuf>,
    #[arg(long)]
    pub active_workline_file: Option<PathBuf>,
    #[arg(long)]
    pub memory_dir: Option<PathBuf>,
    #[arg(long)]
    pub transcript_limit: Option<usize>,
}

#[derive(Debug, Clone, Args)]
pub struct ContinuityThreadIndexEnrichArgs {
    #[arg(long)]
    pub input: PathBuf,
    #[arg(long)]
    pub output: Option<PathBuf>,
}

#[derive(Debug, Clone, Args)]
pub struct ContinuityStartupArgs {
    #[arg(long)]
    pub project: Option<String>,
    #[arg(long)]
    pub repo_root: Option<PathBuf>,
    #[arg(long, default_value = "continuity")]
    pub namespace: String,
}

#[derive(Debug, Clone, Args)]
pub struct ContinuityAnswerArgs {
    #[command(flatten)]
    pub startup: ContinuityStartupArgs,
    #[arg(long)]
    pub question: Option<String>,
    #[arg(long, default_value = "last_chat")]
    pub intent: String,
    #[arg(
        long,
        default_value_t = false,
        alias = "include-previous-chat-messages"
    )]
    pub include_chat_messages: bool,
    #[arg(long, default_value_t = 2)]
    pub messages_count: usize,
    #[arg(long)]
    pub chat_reference: Option<String>,
    #[arg(long)]
    pub at_time_rfc3339: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct ContinuityHandoffArgs {
    #[arg(long)]
    pub project: String,
    #[arg(long, default_value = "continuity")]
    pub namespace: String,
    #[arg(long)]
    pub headline: String,
    #[arg(long = "next-step")]
    pub next_step: String,
    #[arg(long)]
    pub details_file: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct ProjectRegisterArgs {
    #[arg(long)]
    pub code: String,
    #[arg(long)]
    pub display_name: String,
    #[arg(long)]
    pub repo_root: PathBuf,
    #[arg(long)]
    pub default_branch: Option<String>,
}

#[derive(Debug, Args)]
pub struct NamespaceEnsureArgs {
    #[arg(long)]
    pub project: String,
    #[arg(long)]
    pub code: String,
    #[arg(long)]
    pub display_name: Option<String>,
    #[arg(long, default_value = "local_strict")]
    pub retrieval_mode: String,
}

#[derive(Debug, Args)]
pub struct RelationAddArgs {
    #[arg(long)]
    pub source: String,
    #[arg(long)]
    pub target: String,
    #[arg(long)]
    pub relation_type: String,
    #[arg(long)]
    pub shared_contour: String,
    #[arg(long, default_value = "local_plus_related")]
    pub access_mode: String,
}

#[derive(Debug, Clone, Args)]
pub struct ContextPackArgs {
    #[arg(long)]
    pub project: String,
    #[arg(long, default_value = "default")]
    pub namespace: String,
    #[arg(long)]
    pub query: String,
    #[arg(long)]
    pub retrieval_mode: Option<String>,
    #[arg(long, default_value_t = false)]
    pub disable_cache: bool,
    #[arg(long, default_value_t = 5)]
    pub limit_documents: usize,
    #[arg(long, default_value_t = 8)]
    pub limit_symbols: usize,
    #[arg(long, default_value_t = 8)]
    pub limit_chunks: usize,
    #[arg(long, default_value_t = 8)]
    pub limit_semantic_chunks: usize,
}

#[derive(Debug, Clone, Args)]
pub struct WarmupCacheArgs {
    #[arg(long, value_delimiter = ',', num_args = 1..)]
    pub projects: Vec<String>,
    #[arg(long, default_value = "default")]
    pub namespace: String,
    #[arg(long, default_value = "README")]
    pub query: String,
    #[arg(long)]
    pub retrieval_mode: Option<String>,
    #[arg(long, default_value_t = 4)]
    pub limit_documents: usize,
    #[arg(long, default_value_t = 4)]
    pub limit_symbols: usize,
    #[arg(long, default_value_t = 4)]
    pub limit_chunks: usize,
    #[arg(long, default_value_t = 4)]
    pub limit_semantic_chunks: usize,
}

#[derive(Debug, Args)]
pub struct IndexProjectArgs {
    #[arg(long)]
    pub code: String,
    #[arg(long)]
    pub path: PathBuf,
    #[arg(long, default_value = "default")]
    pub namespace: String,
    #[arg(long)]
    pub limit_files: Option<usize>,
    #[arg(long, default_value_t = false)]
    pub skip_embeddings: bool,
}

#[derive(Debug, Clone, Args)]
pub struct VerifyBenchmarkArgs {
    #[command(flatten)]
    pub context: ContextPackArgs,
    #[arg(long, default_value_t = 1)]
    pub warmup: usize,
    #[arg(long, default_value_t = 5)]
    pub iterations: usize,
    #[arg(long, default_value_t = false)]
    pub persist: bool,
    #[arg(long)]
    pub max_mean_ms: Option<u128>,
    #[arg(long)]
    pub max_p95_ms: Option<u128>,
    #[arg(long)]
    pub max_p99_ms: Option<u128>,
    #[arg(long)]
    pub max_max_ms: Option<u128>,
}

#[derive(Debug, Clone, Args)]
pub struct VerifyAccuracyArgs {
    #[arg(long, default_value = "project_alpha")]
    pub project: String,
    #[arg(long, default_value = "project_beta")]
    pub related_project: String,
    #[arg(long, default_value = "review")]
    pub namespace: String,
}

#[derive(Debug, Clone, Args)]
pub struct VerifyLoadArgs {
    #[command(flatten)]
    pub context: ContextPackArgs,
    #[arg(long, default_value_t = 8)]
    pub workers: usize,
    #[arg(long, default_value_t = 25)]
    pub iterations_per_worker: usize,
    #[arg(long, default_value_t = 1)]
    pub warmup_per_worker: usize,
    #[arg(long, default_value_t = false)]
    pub persist: bool,
    #[arg(long)]
    pub max_p95_ms: Option<u128>,
    #[arg(long)]
    pub min_qps: Option<f64>,
    #[arg(long)]
    pub max_error_rate: Option<f64>,
}

#[derive(Debug, Clone, Args)]
pub struct VerifyTokenBenchmarkArgs {
    #[command(flatten)]
    pub context: ContextPackArgs,
    #[arg(long, default_value = "o200k_base")]
    pub tokenizer: String,
    #[arg(long, default_value_t = 200)]
    pub naive_limit_files: usize,
    #[arg(long, default_value_t = 32768)]
    pub naive_max_bytes_per_file: usize,
    #[arg(long, default_value_t = 3.0)]
    pub min_savings_factor: f64,
    #[arg(long, default_value_t = 50.0)]
    pub min_savings_percent: f64,
}

#[derive(Debug, Clone, Args)]
pub struct VerifyTokenBenchmarkSuiteArgs {
    #[arg(long)]
    pub project: String,
    #[arg(long, default_value = "default")]
    pub namespace: String,
    #[arg(long)]
    pub retrieval_mode: Option<String>,
    #[arg(long, action = clap::ArgAction::Append)]
    pub query: Vec<String>,
    #[arg(long)]
    pub queries_file: Option<PathBuf>,
    #[arg(long, default_value_t = false)]
    pub disable_cache: bool,
    #[arg(long, default_value_t = 5)]
    pub limit_documents: usize,
    #[arg(long, default_value_t = 8)]
    pub limit_symbols: usize,
    #[arg(long, default_value_t = 8)]
    pub limit_chunks: usize,
    #[arg(long, default_value_t = 8)]
    pub limit_semantic_chunks: usize,
    #[arg(long, default_value = "o200k_base")]
    pub tokenizer: String,
    #[arg(long, default_value_t = 200)]
    pub naive_limit_files: usize,
    #[arg(long, default_value_t = 32768)]
    pub naive_max_bytes_per_file: usize,
    #[arg(long, default_value_t = 1.2)]
    pub min_mean_savings_factor: f64,
    #[arg(long, default_value_t = 15.0)]
    pub min_mean_savings_percent: f64,
}

#[derive(Debug, Clone, Args)]
pub struct VerifyTextCompareArgs {
    #[arg(long)]
    pub project: String,
    #[arg(long, default_value = "default")]
    pub namespace: String,
    #[arg(long)]
    pub retrieval_mode: Option<String>,
    #[arg(long)]
    pub cases_file: Option<PathBuf>,
    #[arg(long, default_value_t = false)]
    pub disable_cache: bool,
    #[arg(long, default_value_t = 5)]
    pub limit_documents: usize,
    #[arg(long, default_value_t = 8)]
    pub limit_symbols: usize,
    #[arg(long, default_value_t = 8)]
    pub limit_chunks: usize,
    #[arg(long, default_value_t = 8)]
    pub limit_semantic_chunks: usize,
    #[arg(long, default_value = "o200k_base")]
    pub tokenizer: String,
    #[arg(long, default_value_t = 200)]
    pub naive_limit_files: usize,
    #[arg(long, default_value_t = 32768)]
    pub naive_max_bytes_per_file: usize,
    #[arg(long, default_value_t = 1.0)]
    pub min_hybrid_hit_ratio: f64,
    #[arg(long, default_value_t = 1.0)]
    pub min_hybrid_head_hit_ratio: f64,
    #[arg(long, default_value_t = 1.2)]
    pub min_hybrid_savings_factor: f64,
}

#[derive(Debug, Args)]
pub struct VerifyHostileArgs {
    #[arg(long, default_value = "all")]
    pub scenario: String,
}

#[derive(Debug, Clone, Args)]
pub struct VerifyMcpArgs {
    #[command(flatten)]
    pub context: ContextPackArgs,
    #[arg(long, default_value = "o200k_base")]
    pub tokenizer: String,
    #[arg(long, default_value_t = 20)]
    pub naive_limit_files: usize,
    #[arg(long, default_value_t = 32768)]
    pub naive_max_bytes_per_file: usize,
    #[arg(long, default_value_t = 1.2)]
    pub min_savings_factor: f64,
    #[arg(long, default_value_t = 15.0)]
    pub min_savings_percent: f64,
}

#[derive(Debug, Clone, Args)]
pub struct ObserveServeArgs {
    #[arg(long, default_value = "0.0.0.0:9464")]
    pub bind: String,
}

#[derive(Debug, Clone, Args)]
pub struct ObserveTokenReportArgs {
    #[arg(long)]
    pub budget_profile: Option<String>,
    #[arg(long)]
    pub include_verify_events: Option<bool>,
}

#[derive(Debug, Clone, Args)]
pub struct ObserveRepairTokenLedgerArgs {
    #[arg(long, default_value_t = false)]
    pub apply: bool,
    #[arg(long)]
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Args)]
pub struct ObserveReverifyTokenLedgerArgs {
    #[arg(long, default_value_t = false)]
    pub apply: bool,
    #[arg(long)]
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Args)]
pub struct BootstrapStackArgs {
    #[arg(long, default_value = "default")]
    pub stack_profile: String,
}

#[derive(Debug, Clone, Args)]
pub struct BootstrapPreflightArgs {
    #[arg(long, default_value = "default")]
    pub stack_profile: String,
}

#[derive(Debug, Clone, Args)]
pub struct McpConfigArgs {
    #[arg(long, default_value = "generic")]
    pub client: String,
    #[arg(long, default_value = "amai")]
    pub server_name: String,
    #[arg(long, default_value = "auto")]
    pub launcher_platform: String,
    #[arg(long)]
    pub ssh_destination: Option<String>,
    #[arg(long)]
    pub remote_repo_root: Option<PathBuf>,
    #[arg(long)]
    pub command: Option<String>,
    #[arg(long)]
    pub cwd: Option<PathBuf>,
    #[arg(long)]
    pub output: Option<PathBuf>,
}

#[derive(Debug, Clone, Args)]
pub struct BootstrapOnboardingArgs {
    #[arg(long, default_value = "auto")]
    pub client: String,
    #[arg(long, default_value = "default")]
    pub stack_profile: String,
    #[arg(long, default_value_t = false)]
    pub yes: bool,
    #[arg(long, default_value = "auto")]
    pub launcher_platform: String,
    #[arg(long)]
    pub ssh_destination: Option<String>,
    #[arg(long)]
    pub remote_repo_root: Option<PathBuf>,
    #[arg(long)]
    pub output: Option<PathBuf>,
    #[arg(long)]
    pub cwd: Option<PathBuf>,
    #[arg(long, default_value_t = false)]
    pub skip_release_build: bool,
    #[arg(long, default_value_t = false)]
    pub skip_stack: bool,
}

#[derive(Debug, Clone, Args)]
pub struct BootstrapDisconnectArgs {
    #[arg(long, default_value = "auto")]
    pub client: String,
    #[arg(long)]
    pub output: Option<PathBuf>,
    #[arg(long)]
    pub cwd: Option<PathBuf>,
    #[arg(long, default_value_t = true)]
    pub purge_empty_file: bool,
}
