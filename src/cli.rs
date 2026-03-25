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
    ExternalCheck,
    ExternalExplain(BenchmarkArgs),
    ExternalDatasets,
    ExternalDownload(BenchmarkDatasetArgs),
    ExternalPlan(BenchmarkArgs),
    ExternalAdapter(BenchmarkExternalAdapterArgs),
    ExternalHarvest(BenchmarkExternalHarvestArgs),
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
    ColdPath(Box<VerifyColdPathArgs>),
    TokenBenchmark(Box<VerifyTokenBenchmarkArgs>),
    TokenBenchmarkSuite(Box<VerifyTokenBenchmarkSuiteArgs>),
    TextCompare(Box<VerifyTextCompareArgs>),
    McpMatrix(Box<VerifyMcpMatrixArgs>),
    MemoryMatrix(Box<VerifyMemoryMatrixArgs>),
    Continuity(Box<VerifyContinuityArgs>),
    Accuracy(VerifyAccuracyArgs),
    Degradation(VerifyDegradationArgs),
    Load(Box<VerifyLoadArgs>),
    Hostile(VerifyHostileArgs),
    Mcp(Box<VerifyMcpArgs>),
}

#[derive(Debug, Subcommand)]
pub enum ObserveCommand {
    Snapshot,
    SlaCheck,
    Guardrails,
    TokenReport(ObserveTokenReportArgs),
    TokenEvidencePack(ObserveTokenEvidencePackArgs),
    TokenContractualSources(ObserveTokenContractualSourcesArgs),
    TokenStatementExport(ObserveTokenStatementExportArgs),
    TokenAdjustmentRegistry(ObserveTokenAdjustmentRegistryArgs),
    TokenAdjustmentAdd(ObserveTokenAdjustmentAddArgs),
    TokenWholeCycleAttach(ObserveTokenWholeCycleAttachArgs),
    TokenWholeCycleTurnAttach(ObserveTokenWholeCycleTurnAttachArgs),
    TokenRolloutAssistantGeneration(ObserveTokenRolloutAssistantGenerationArgs),
    CleanupSnapshots(ObserveCleanupSnapshotsArgs),
    CleanupArtifacts(ObserveCleanupArtifactsArgs),
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
pub struct BenchmarkDatasetArgs {
    #[arg(long)]
    pub dataset: Option<String>,
    #[arg(long, default_value_t = false)]
    pub force: bool,
}

#[derive(Debug, Clone, Args)]
pub struct BenchmarkExternalAdapterArgs {
    #[arg(long)]
    pub benchmark: String,
    #[arg(long)]
    pub dataset: String,
    #[arg(long, default_value_t = false)]
    pub download_missing: bool,
    #[arg(long)]
    pub output_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Args)]
pub struct BenchmarkExternalHarvestArgs {
    #[arg(long)]
    pub benchmark: String,
    #[arg(long)]
    pub dataset: String,
    #[arg(long)]
    pub output_dir: Option<PathBuf>,
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
    #[arg(long, default_value_t = false)]
    pub json: bool,
    #[arg(
        long,
        default_value = "live_continuity_startup",
        help = "Token ledger source kind for continuity-startup observed whole-cycle events. Use proof_/verify_ prefixes for engineering runs."
    )]
    pub token_source_kind: String,
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
    #[arg(
        long,
        default_value = "live_context_pack",
        help = "Token ledger source kind for this context-pack call. Use proof_/verify_ prefixes for engineering runs so they do not contaminate live tokenonomics."
    )]
    pub token_source_kind: String,
    #[arg(
        long,
        help = "Optional whole-cycle override for actual client-side prompt tokens in the same meter the upstream client/provider reports."
    )]
    pub client_prompt_tokens: Option<u64>,
    #[arg(
        long,
        help = "Optional whole-cycle observed assistant generation tokens for this context-pack event."
    )]
    pub assistant_generation_tokens: Option<u64>,
    #[arg(
        long,
        help = "Optional whole-cycle observed non-retrieval tool overhead tokens for this context-pack event."
    )]
    pub tool_overhead_tokens: Option<u64>,
    #[arg(
        long,
        help = "Optional whole-cycle observed continuity-restore tokens outside retrieval for this context-pack event."
    )]
    pub continuity_restore_tokens: Option<u64>,
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
    #[arg(
        long,
        help = "Optional newline-delimited file with exact relative paths to index deterministically"
    )]
    pub paths_file: Option<PathBuf>,
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
pub struct VerifyColdPathArgs {
    #[arg(
        long,
        default_value = "config/cold_benchmark_manifest.toml",
        help = "Path to the machine-readable cold benchmark dataset manifest"
    )]
    pub manifest: PathBuf,
    #[arg(
        long,
        default_value_t = 2,
        help = "How many full dataset cycles to run for the long-run contour"
    )]
    pub cycles: usize,
    #[arg(
        long,
        default_value_t = 85.0,
        help = "Thermal guard in Celsius. When exceeded, the runner pauses or stops."
    )]
    pub thermal_guard_celsius: f64,
    #[arg(
        long,
        default_value_t = 20,
        help = "How many seconds to cool down before retrying after a thermal spike"
    )]
    pub cooldown_seconds: u64,
    #[arg(
        long,
        default_value_t = 2,
        help = "How many cooldown retries are allowed before the run stops"
    )]
    pub max_cooldown_retries: usize,
    #[arg(
        long,
        default_value_t = 25.0,
        help = "Minimum free disk GiB required to continue the run"
    )]
    pub min_disk_free_gib: f64,
    #[arg(
        long,
        default_value_t = false,
        help = "Reindex every repo on every cycle instead of only before the first cycle"
    )]
    pub reindex_each_cycle: bool,
    #[arg(
        long,
        default_value_t = false,
        help = "Skip indexing and assume the manifest repos are already indexed"
    )]
    pub skip_index: bool,
    #[arg(
        long,
        default_value = "state/cold-benchmark/latest",
        help = "Directory for report, JSON summary and per-sample CSV"
    )]
    pub output_dir: PathBuf,
}

#[derive(Debug, Clone, Args)]
pub struct VerifyAccuracyArgs {
    #[arg(long, default_value = "project_alpha")]
    pub project: String,
    #[arg(long, default_value = "project_beta")]
    pub related_project: String,
    #[arg(long, default_value = "review")]
    pub namespace: String,
    #[arg(
        long,
        default_value = "config/red_team_retrieval_isolation.toml",
        help = "Path to the machine-readable red-team retrieval isolation suite manifest"
    )]
    pub manifest: PathBuf,
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
    #[arg(long, default_value_t = false)]
    pub record_live_context: bool,
}

#[derive(Debug, Clone, Args)]
pub struct VerifyDegradationArgs {
    #[arg(long, default_value = "all")]
    pub scenario: String,
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
pub struct VerifyMcpMatrixArgs {
    #[arg(long, default_value = "live_mcpbench_local")]
    pub matrix: String,
    #[arg(long, default_value = "project_alpha")]
    pub project: String,
    #[arg(long, default_value = "project_beta")]
    pub related_project: String,
    #[arg(long, default_value = "review")]
    pub namespace: String,
    #[arg(long, default_value = "codex_5h")]
    pub budget_profile: String,
    #[arg(long)]
    pub min_success_rate: Option<f64>,
    #[arg(long)]
    pub max_p95_ms: Option<f64>,
}

#[derive(Debug, Clone, Args)]
pub struct VerifyMemoryMatrixArgs {
    #[arg(long, default_value = "letta_memory_local")]
    pub matrix: String,
    #[arg(long, default_value = "memory_eval")]
    pub project_prefix: String,
    #[arg(long)]
    pub min_success_rate: Option<f64>,
    #[arg(long)]
    pub min_mean_score: Option<f64>,
    #[arg(long)]
    pub max_p95_ms: Option<f64>,
}

#[derive(Debug, Clone, Args)]
pub struct VerifyContinuityArgs {
    #[arg(long)]
    pub project: Option<String>,
    #[arg(long)]
    pub repo_root: Option<PathBuf>,
    #[arg(long, default_value = "continuity")]
    pub namespace: String,
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
pub struct ObserveTokenEvidencePackArgs {
    #[arg(long, default_value = "lifetime")]
    pub scope: String,
    #[arg(long)]
    pub budget_profile: Option<String>,
    #[arg(long)]
    pub include_verify_events: Option<bool>,
    #[arg(long)]
    pub output: Option<PathBuf>,
}

#[derive(Debug, Clone, Args)]
pub struct ObserveTokenContractualSourcesArgs {
    #[arg(long, default_value = "rolling_window")]
    pub scope: String,
    #[arg(long)]
    pub budget_profile: Option<String>,
    #[arg(long)]
    pub include_verify_events: Option<bool>,
}

#[derive(Debug, Clone, Args)]
pub struct ObserveTokenStatementExportArgs {
    #[arg(long, default_value = "lifetime")]
    pub scope: String,
    #[arg(long)]
    pub budget_profile: Option<String>,
    #[arg(long)]
    pub include_verify_events: Option<bool>,
    #[arg(long)]
    pub output_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Args)]
pub struct ObserveTokenAdjustmentRegistryArgs {
    #[arg(long)]
    pub scope: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct ObserveTokenAdjustmentAddArgs {
    #[arg(long)]
    pub scope: String,
    #[arg(long, default_value = "adjustment_entry")]
    pub kind: String,
    #[arg(long, default_value = "requested")]
    pub status: String,
    #[arg(long)]
    pub reason_code: String,
    #[arg(long)]
    pub tokens_delta: Option<i64>,
    #[arg(long)]
    pub amount_delta: Option<f64>,
    #[arg(long)]
    pub currency_profile: Option<String>,
    #[arg(long)]
    pub related_statement_id: Option<String>,
    #[arg(long, default_value_t = false)]
    pub resolve_related_statement_id: bool,
    #[arg(long)]
    pub budget_profile: Option<String>,
    #[arg(long)]
    pub include_verify_events: Option<bool>,
    #[arg(long)]
    pub adjustment_id: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct ObserveTokenWholeCycleAttachArgs {
    #[arg(long)]
    pub context_pack_id: String,
    #[arg(long)]
    pub client_prompt_tokens: Option<u64>,
    #[arg(long)]
    pub assistant_generation_tokens: Option<u64>,
    #[arg(long)]
    pub tool_overhead_tokens: Option<u64>,
    #[arg(long)]
    pub continuity_restore_tokens: Option<u64>,
}

#[derive(Debug, Clone, Args)]
pub struct ObserveTokenWholeCycleTurnAttachArgs {
    #[arg(long)]
    pub thread_id: String,
    #[arg(long)]
    pub turn_id: String,
    #[arg(long = "context-pack-id", required = true)]
    pub context_pack_ids: Vec<String>,
    #[arg(long)]
    pub assistant_generation_tokens: u64,
}

#[derive(Debug, Clone, Args)]
pub struct ObserveTokenRolloutAssistantGenerationArgs {
    #[arg(long)]
    pub rollout_path: Option<PathBuf>,
    #[arg(long)]
    pub repo_root: Option<PathBuf>,
    #[arg(long, default_value_t = false)]
    pub apply: bool,
}

#[derive(Debug, Clone, Args)]
pub struct ObserveCleanupSnapshotsArgs {
    #[arg(long, default_value_t = false)]
    pub apply: bool,
    #[arg(long)]
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Args)]
pub struct ObserveCleanupArtifactsArgs {
    #[arg(long, default_value_t = false)]
    pub apply: bool,
    #[arg(long)]
    pub limit: Option<usize>,
    #[arg(long, default_value_t = false)]
    pub aggressive: bool,
}

#[derive(Debug, Clone, Args)]
pub struct ObserveRepairTokenLedgerArgs {
    #[arg(long, default_value_t = false)]
    pub apply: bool,
    #[arg(long)]
    pub limit: Option<i64>,
    #[arg(long)]
    pub project: Option<String>,
    #[arg(long)]
    pub project_prefix: Option<String>,
    #[arg(long)]
    pub namespace: Option<String>,
    #[arg(long)]
    pub source_kind: Option<String>,
    #[arg(long)]
    pub rewrite_source_kind: Option<String>,
    #[arg(long)]
    pub repair_reason: Option<String>,
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
