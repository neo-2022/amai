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
}

#[derive(Debug, Subcommand)]
pub enum BootstrapCommand {
    Stack,
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
}

#[derive(Debug, Subcommand)]
pub enum IndexCommand {
    Project(IndexProjectArgs),
}

#[derive(Debug, Subcommand)]
pub enum VerifyCommand {
    Benchmark(Box<VerifyBenchmarkArgs>),
    Hostile(VerifyHostileArgs),
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

#[derive(Debug, Args)]
pub struct ContextPackArgs {
    #[arg(long)]
    pub project: String,
    #[arg(long, default_value = "default")]
    pub namespace: String,
    #[arg(long)]
    pub query: String,
    #[arg(long)]
    pub retrieval_mode: Option<String>,
    #[arg(long, default_value_t = 5)]
    pub limit_documents: usize,
    #[arg(long, default_value_t = 8)]
    pub limit_symbols: usize,
    #[arg(long, default_value_t = 8)]
    pub limit_chunks: usize,
    #[arg(long, default_value_t = 8)]
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

#[derive(Debug, Args)]
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
    pub max_max_ms: Option<u128>,
}

#[derive(Debug, Args)]
pub struct VerifyHostileArgs {
    #[arg(long, default_value = "all")]
    pub scenario: String,
}
