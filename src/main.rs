#![recursion_limit = "256"]

mod artifact_cleanup;
mod benchmark_matrix;
mod bootstrap;
mod chat_question;
mod cli;
mod codex_threads;
mod cold_benchmark;
mod compatibility;
mod config;
mod continuity;
mod dashboard;
mod degradation_proof;
mod deployment;
mod edge_cache;
mod eval_verdict;
mod external_benchmark;
mod external_benchmark_conversion;
mod hardware_telemetry;
mod indexer;
mod language;
mod mcp;
mod mcp_task_matrix;
mod memory_task_matrix;
mod nats;
mod observability_policy;
mod observe;
mod onboarding;
mod postgres;
mod profiles;
mod qdrant;
mod retrieval;
mod retrieval_science;
mod s3;
mod status;
mod syntax;
mod token_budget;
mod verify;
mod warmup;
mod working_state;
mod workspace_graph;

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
                BenchmarkCommand::ExternalCheck => {
                    external_benchmark::print_external_check(&repo_root)?
                }
                BenchmarkCommand::ExternalExplain(args) => {
                    external_benchmark::print_external_explainer(&repo_root, &args.benchmark)?
                }
                BenchmarkCommand::ExternalDatasets => {
                    external_benchmark::print_external_datasets(&repo_root)?
                }
                BenchmarkCommand::ExternalDownload(args) => {
                    external_benchmark::download_datasets(
                        &repo_root,
                        args.dataset.as_deref(),
                        args.force,
                    )
                    .await?
                }
                BenchmarkCommand::ExternalPlan(args) => {
                    external_benchmark::print_external_plan(&repo_root, &args.benchmark)?
                }
                BenchmarkCommand::ExternalAdapter(args) => {
                    external_benchmark::run_external_adapter(
                        &repo_root,
                        &args.benchmark,
                        &args.dataset,
                        args.download_missing,
                        args.output_dir.as_deref(),
                    )
                    .await?
                }
                BenchmarkCommand::ExternalHarvest(args) => {
                    external_benchmark::print_external_harvest(
                        &repo_root,
                        &args.benchmark,
                        &args.dataset,
                        args.output_dir.as_deref(),
                    )?
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
            ContinuityCommand::StartupState(args) => {
                continuity::print_startup_runtime_state(&args)?;
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
            ContinuityCommand::RotateChat(args) => {
                let cfg = config::AppConfig::from_env()?;
                compatibility::assert_supported(&cfg).await?;
                continuity::rotate_chat(&cfg, &args).await?;
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
                        "ast_eligible_files": stats.ast_eligible_files,
                        "files_with_ast": stats.files_with_ast,
                        "files_with_lexical_fallback": stats.files_with_lexical_fallback,
                        "files_without_ast_support": stats.files_without_ast_support,
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
            VerifyCommand::ColdPath(args) => {
                let cfg = config::AppConfig::from_env()?;
                compatibility::assert_supported(&cfg).await?;
                let mut db = postgres::connect_admin(&cfg).await?;
                cold_benchmark::run(&cfg, &mut db, &args).await?;
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
            VerifyCommand::Continuity(args) => {
                let cfg = config::AppConfig::from_env()?;
                compatibility::assert_supported(&cfg).await?;
                continuity::verify_continuity(&cfg, &args).await?;
            }
            VerifyCommand::Accuracy(args) => {
                let cfg = config::AppConfig::from_env()?;
                compatibility::assert_supported(&cfg).await?;
                let mut db = postgres::connect_admin(&cfg).await?;
                verify::run_accuracy(&cfg, &mut db, &args).await?;
            }
            VerifyCommand::Degradation(args) => {
                let cfg = config::AppConfig::from_env()?;
                compatibility::assert_supported(&cfg).await?;
                let mut db = postgres::connect_admin(&cfg).await?;
                verify::run_degradation(&cfg, &mut db, &args).await?;
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
            ObserveCommand::Guardrails => {
                let cfg = config::AppConfig::from_env()?;
                compatibility::assert_supported(&cfg).await?;
                observe::print_guardrails(&cfg).await?;
            }
            ObserveCommand::ClientBudgetGuard(args) => {
                let cfg = config::AppConfig::from_env()?;
                compatibility::assert_supported(&cfg).await?;
                observe::print_client_budget_guard(&cfg, args.enforce_reply_gate).await?;
            }
            ObserveCommand::TokenReport(args) => {
                let cfg = config::AppConfig::from_env()?;
                compatibility::assert_supported(&cfg).await?;
                let db = postgres::connect_admin(&cfg).await?;
                token_budget::print_report(&db, &args).await?;
            }
            ObserveCommand::TokenEvidencePack(args) => {
                let cfg = config::AppConfig::from_env()?;
                compatibility::assert_supported(&cfg).await?;
                let db = postgres::connect_admin(&cfg).await?;
                token_budget::print_evidence_pack(&db, &args).await?;
            }
            ObserveCommand::TokenContractualSources(args) => {
                let cfg = config::AppConfig::from_env()?;
                compatibility::assert_supported(&cfg).await?;
                let db = postgres::connect_admin(&cfg).await?;
                token_budget::print_contractual_sources(&db, &args).await?;
            }
            ObserveCommand::TokenStatementExport(args) => {
                let cfg = config::AppConfig::from_env()?;
                compatibility::assert_supported(&cfg).await?;
                let db = postgres::connect_admin(&cfg).await?;
                token_budget::print_statement_export_bundle(&db, &args).await?;
            }
            ObserveCommand::TokenAdjustmentRegistry(args) => {
                let cfg = config::AppConfig::from_env()?;
                compatibility::assert_supported(&cfg).await?;
                token_budget::print_adjustment_registry(&args).await?;
            }
            ObserveCommand::TokenAdjustmentAdd(args) => {
                let cfg = config::AppConfig::from_env()?;
                compatibility::assert_supported(&cfg).await?;
                token_budget::add_adjustment_entry(&args).await?;
            }
            ObserveCommand::TokenWholeCycleAttach(args) => {
                let cfg = config::AppConfig::from_env()?;
                compatibility::assert_supported(&cfg).await?;
                let db = postgres::connect_admin(&cfg).await?;
                token_budget::attach_whole_cycle_observed_for_context_pack(&db, &args).await?;
            }
            ObserveCommand::TokenWholeCycleTurnAttach(args) => {
                let cfg = config::AppConfig::from_env()?;
                compatibility::assert_supported(&cfg).await?;
                let db = postgres::connect_admin(&cfg).await?;
                token_budget::attach_whole_cycle_observed_for_turn_group(&db, &args).await?;
            }
            ObserveCommand::TokenRolloutAssistantGeneration(args) => {
                let cfg = config::AppConfig::from_env()?;
                compatibility::assert_supported(&cfg).await?;
                let db = postgres::connect_admin(&cfg).await?;
                token_budget::observe_rollout_assistant_generation(&db, &args).await?;
            }
            ObserveCommand::CleanupSnapshots(args) => {
                let cfg = config::AppConfig::from_env()?;
                compatibility::assert_supported(&cfg).await?;
                observe::print_retention_cleanup(&cfg, args.apply, args.limit).await?;
            }
            ObserveCommand::CleanupArtifacts(args) => {
                let cfg = config::AppConfig::from_env()?;
                compatibility::assert_supported(&cfg).await?;
                observe::print_artifact_cleanup(
                    &cfg,
                    args.apply,
                    args.limit,
                    args.aggressive,
                    args.target.as_deref(),
                )
                .await?;
            }
            ObserveCommand::RepairTokenLedger(args) => {
                let cfg = config::AppConfig::from_env()?;
                compatibility::assert_supported(&cfg).await?;
                let db = postgres::connect_admin(&cfg).await?;
                token_budget::repair_token_ledger_events(
                    &db,
                    args.apply,
                    token_budget::TokenLedgerRepairRequest {
                        limit: args.limit,
                        project: args.project,
                        project_prefix: args.project_prefix,
                        namespace: args.namespace,
                        source_kind: args.source_kind,
                        correlation_id: args.correlation_id,
                        rewrite_source_kind: args.rewrite_source_kind,
                        repair_reason: args.repair_reason,
                    },
                )
                .await?;
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
