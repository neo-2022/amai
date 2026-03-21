#![recursion_limit = "256"]

mod benchmark_matrix;
mod bootstrap;
mod chat_question;
mod cli;
mod codex_threads;
mod compatibility;
mod config;
mod continuity;
mod dashboard;
mod deployment;
mod edge_cache;
mod indexer;
mod language;
mod mcp;
mod mcp_task_matrix;
mod memory_task_matrix;
mod nats;
mod observe;
mod onboarding;
mod postgres;
mod profiles;
mod qdrant;
mod retrieval;
mod s3;
mod status;
mod syntax;
mod token_budget;
mod verify;
mod warmup;
mod working_state;

use anyhow::Result;
use clap::Parser;
use cli::{
    BenchmarkCommand, BootstrapCommand, Cli, Command, CompatCommand, ContextCommand,
    ContinuityCommand, DeploymentCommand, IndexCommand, McpCommand, NamespaceCommand,
    ObserveCommand, ProjectCommand, RelationCommand, VerifyCommand,
};
use std::path::Path;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    load_env_contour();
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_target(false)
        .compact()
        .init();

    let cli = Cli::parse();

    match cli.command {
        Command::Benchmark { command } => {
            let repo_root = config::discover_repo_root(None)?;
            match command {
                BenchmarkCommand::List => benchmark_matrix::print_matrix(&repo_root)?,
                BenchmarkCommand::Coverage => benchmark_matrix::print_coverage(&repo_root)?,
                BenchmarkCommand::Explain(args) => {
                    benchmark_matrix::print_benchmark_explainer(&repo_root, &args.benchmark)?
                }
            }
        }
        Command::Continuity { command } => match command {
            ContinuityCommand::Import(args) => {
                let cfg = config::AppConfig::from_env()?;
                compatibility::assert_supported(&cfg).await?;
                continuity::import_sources(&cfg, &args).await?;
            }
            ContinuityCommand::EnrichThreadIndex(args) => {
                continuity::enrich_thread_index_file(&args)?;
            }
            ContinuityCommand::Startup(args) => {
                let cfg = config::AppConfig::from_env()?;
                compatibility::assert_supported(&cfg).await?;
                continuity::print_startup(&cfg, &args).await?;
            }
            ContinuityCommand::Restore(args) => {
                let cfg = config::AppConfig::from_env()?;
                compatibility::assert_supported(&cfg).await?;
                continuity::print_restore(&cfg, &args).await?;
            }
            ContinuityCommand::Answer(args) => {
                let cfg = config::AppConfig::from_env()?;
                compatibility::assert_supported(&cfg).await?;
                continuity::print_answer(&cfg, &args).await?;
            }
            ContinuityCommand::Handoff(args) => {
                let cfg = config::AppConfig::from_env()?;
                compatibility::assert_supported(&cfg).await?;
                continuity::capture_handoff(&cfg, &args).await?;
            }
        },
        Command::Deployment { command } => {
            let repo_root = config::discover_repo_root(None)?;
            match command {
                DeploymentCommand::List => deployment::print_targets(&repo_root)?,
                DeploymentCommand::Explain(args) => {
                    deployment::print_target_explainer(&repo_root, &args.target)?
                }
                DeploymentCommand::Preflight(args) => {
                    deployment::print_target_preflight(&repo_root, &args.target)?
                }
            }
        }
        Command::Bootstrap { command } => match command {
            BootstrapCommand::Stack(_args) => {
                let cfg = config::AppConfig::from_env()?;
                bootstrap::bootstrap_stack(&cfg).await?
            }
            BootstrapCommand::Preflight(args) => {
                let repo_root = config::discover_repo_root(None)?;
                profiles::print_preflight(&repo_root, &args.stack_profile)?;
            }
            BootstrapCommand::Install(args) => onboarding::run(&args).await?,
            BootstrapCommand::Onboarding(args) => onboarding::run(&args).await?,
            BootstrapCommand::Remove(args) => onboarding::disconnect(&args).await?,
            BootstrapCommand::Disconnect(args) => onboarding::disconnect(&args).await?,
        },
        Command::Compat { command } => match command {
            CompatCommand::Check => {
                let cfg = config::AppConfig::from_env()?;
                compatibility::print_report(&cfg).await?
            }
        },
        Command::Status => {
            let cfg = config::AppConfig::from_env()?;
            status::print_status(&cfg).await?
        }
        Command::Project { command } => {
            let cfg = config::AppConfig::from_env()?;
            let db = postgres::connect_admin(&cfg).await?;
            match command {
                ProjectCommand::Register(args) => {
                    let project = postgres::upsert_project(
                        &db,
                        &args.code,
                        &args.display_name,
                        &args.repo_root.display().to_string(),
                        args.default_branch.as_deref(),
                        &cfg.default_retrieval_mode,
                    )
                    .await?;
                    println!(
                        "project registered: {} ({}) -> {}",
                        project.code, project.display_name, project.repo_root
                    );
                }
                ProjectCommand::List => {
                    for project in postgres::list_projects(&db).await? {
                        println!(
                            "{} :: {} :: {}",
                            project.code, project.display_name, project.repo_root
                        );
                    }
                }
            }
        }
        Command::Namespace { command } => {
            let cfg = config::AppConfig::from_env()?;
            let db = postgres::connect_admin(&cfg).await?;
            match command {
                NamespaceCommand::Ensure(args) => {
                    let project = postgres::get_project_by_code(&db, &args.project).await?;
                    let namespace = postgres::ensure_namespace(
                        &db,
                        project.project_id,
                        &args.code,
                        args.display_name.as_deref(),
                        &args.retrieval_mode,
                    )
                    .await?;
                    println!("namespace ensured: {} :: {}", project.code, namespace.code);
                }
            }
        }
        Command::Relation { command } => {
            let cfg = config::AppConfig::from_env()?;
            let db = postgres::connect_admin(&cfg).await?;
            match command {
                RelationCommand::Add(args) => {
                    postgres::add_relation(
                        &db,
                        &args.source,
                        &args.target,
                        &args.relation_type,
                        &args.shared_contour,
                        &args.access_mode,
                    )
                    .await?;
                    println!(
                        "relation ensured: {} -> {} [{} / {} / {}]",
                        args.source,
                        args.target,
                        args.relation_type,
                        args.shared_contour,
                        args.access_mode
                    );
                }
            }
        }
        Command::Context { command } => {
            let cfg = config::AppConfig::from_env()?;
            compatibility::assert_supported(&cfg).await?;
            let mut db = postgres::connect_admin(&cfg).await?;
            match command {
                ContextCommand::Pack(args) => {
                    retrieval::build_context_pack(&cfg, &mut db, &args).await?
                }
                ContextCommand::Warm(args) => warmup::run(&cfg, &mut db, &args).await?,
            }
        }
        Command::Index { command } => match command {
            IndexCommand::Project(args) => {
                let cfg = config::AppConfig::from_env()?;
                compatibility::assert_supported(&cfg).await?;
                let mut db = postgres::connect_admin(&cfg).await?;
                let stats = indexer::index_project(&cfg, &mut db, &args).await?;
                let payload = serde_json::json!({
                    "index_project": {
                        "code": args.code,
                        "namespace": args.namespace,
                        "path": args.path.display().to_string(),
                        "skip_embeddings": args.skip_embeddings,
                        "limit_files": args.limit_files,
                        "files_indexed": stats.files_indexed,
                        "files_with_ast": stats.files_with_ast,
                        "files_with_lexical_fallback": stats.files_with_lexical_fallback,
                        "symbols_written": stats.symbols_written,
                        "chunks_written": stats.chunks_written,
                        "vector_points_written": stats.vector_points_written,
                        "total_bytes": stats.total_bytes,
                        "elapsed_ms": stats.elapsed_ms,
                        "files_per_min": stats.files_per_min,
                        "parser_coverage_ratio": stats.parser_coverage_ratio,
                        "language_breakdown": stats.language_breakdown
                    }
                });
                let _ =
                    postgres::insert_observability_snapshot(&db, "index_project", &payload).await?;
                println!("{}", serde_json::to_string_pretty(&payload)?);
            }
        },
        Command::Verify { command } => match command {
            VerifyCommand::Benchmark(args) => {
                let cfg = config::AppConfig::from_env()?;
                compatibility::assert_supported(&cfg).await?;
                let mut db = postgres::connect_admin(&cfg).await?;
                verify::run_benchmark(&cfg, &mut db, &args).await?;
            }
            VerifyCommand::TokenBenchmark(args) => {
                let cfg = config::AppConfig::from_env()?;
                compatibility::assert_supported(&cfg).await?;
                let mut db = postgres::connect_admin(&cfg).await?;
                verify::run_token_benchmark(&cfg, &mut db, &args).await?;
            }
            VerifyCommand::TokenBenchmarkSuite(args) => {
                let cfg = config::AppConfig::from_env()?;
                compatibility::assert_supported(&cfg).await?;
                let mut db = postgres::connect_admin(&cfg).await?;
                verify::run_token_benchmark_suite(&cfg, &mut db, &args).await?;
            }
            VerifyCommand::TextCompare(args) => {
                let cfg = config::AppConfig::from_env()?;
                compatibility::assert_supported(&cfg).await?;
                let mut db = postgres::connect_admin(&cfg).await?;
                verify::run_text_compare(&cfg, &mut db, &args).await?;
            }
            VerifyCommand::McpMatrix(args) => {
                let cfg = config::AppConfig::from_env()?;
                compatibility::assert_supported(&cfg).await?;
                mcp_task_matrix::run_matrix(&cfg, &args).await?;
            }
            VerifyCommand::MemoryMatrix(args) => {
                let cfg = config::AppConfig::from_env()?;
                compatibility::assert_supported(&cfg).await?;
                memory_task_matrix::run_matrix(&cfg, &args).await?;
            }
            VerifyCommand::Accuracy(args) => {
                let cfg = config::AppConfig::from_env()?;
                compatibility::assert_supported(&cfg).await?;
                let mut db = postgres::connect_admin(&cfg).await?;
                verify::run_accuracy(&cfg, &mut db, &args).await?;
            }
            VerifyCommand::Load(args) => {
                let cfg = config::AppConfig::from_env()?;
                compatibility::assert_supported(&cfg).await?;
                verify::run_load(&cfg, &args).await?;
            }
            VerifyCommand::Hostile(args) => {
                let cfg = config::AppConfig::from_env()?;
                verify::run_hostile(&cfg, &args).await?;
            }
            VerifyCommand::Mcp(args) => {
                let cfg = config::AppConfig::from_env()?;
                compatibility::assert_supported(&cfg).await?;
                mcp::run_smoke_proof(&cfg, &args).await?;
            }
        },
        Command::Observe { command } => match command {
            ObserveCommand::Snapshot => {
                let cfg = config::AppConfig::from_env()?;
                compatibility::assert_supported(&cfg).await?;
                observe::print_snapshot(&cfg).await?;
            }
            ObserveCommand::SlaCheck => {
                let cfg = config::AppConfig::from_env()?;
                compatibility::assert_supported(&cfg).await?;
                observe::run_sla_check(&cfg).await?;
            }
            ObserveCommand::TokenReport(args) => {
                let cfg = config::AppConfig::from_env()?;
                compatibility::assert_supported(&cfg).await?;
                let db = postgres::connect_admin(&cfg).await?;
                token_budget::print_report(&db, &args).await?;
            }
            ObserveCommand::RepairTokenLedger(args) => {
                let cfg = config::AppConfig::from_env()?;
                compatibility::assert_supported(&cfg).await?;
                let db = postgres::connect_admin(&cfg).await?;
                token_budget::repair_legacy_token_events(&db, args.apply, args.limit).await?;
            }
            ObserveCommand::ReverifyTokenLedger(args) => {
                let cfg = config::AppConfig::from_env()?;
                compatibility::assert_supported(&cfg).await?;
                let mut db = postgres::connect_admin(&cfg).await?;
                token_budget::reverify_legacy_live_events(&cfg, &mut db, args.apply, args.limit)
                    .await?;
            }
            ObserveCommand::Serve(args) => {
                let cfg = config::AppConfig::from_env()?;
                compatibility::assert_supported(&cfg).await?;
                observe::serve_metrics(&cfg, &args.bind).await?;
            }
        },
        Command::Mcp { command } => match command {
            McpCommand::Serve => {
                let cfg = config::AppConfig::from_env()?;
                mcp::serve(&cfg).await?;
            }
            McpCommand::Config(args) => {
                mcp::write_client_config(&args)?;
            }
        },
    }

    Ok(())
}

fn load_env_contour() {
    dotenvy::dotenv().ok();
    if std::env::var_os("AMI_STACK_NAME").is_some() {
        return;
    }
    let manifest_env = Path::new(env!("CARGO_MANIFEST_DIR")).join(".env");
    dotenvy::from_path_override(&manifest_env).ok();
}
