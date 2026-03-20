mod bootstrap;
mod cli;
mod config;
mod edge_cache;
mod indexer;
mod language;
mod nats;
mod postgres;
mod qdrant;
mod retrieval;
mod s3;
mod status;
mod syntax;

use anyhow::Result;
use clap::Parser;
use cli::{
    BootstrapCommand, Cli, Command, ContextCommand, IndexCommand, NamespaceCommand, ProjectCommand,
    RelationCommand,
};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_target(false)
        .compact()
        .init();

    let cli = Cli::parse();
    let cfg = config::AppConfig::from_env()?;

    match cli.command {
        Command::Bootstrap { command } => match command {
            BootstrapCommand::Stack => bootstrap::bootstrap_stack(&cfg).await?,
        },
        Command::Status => status::print_status(&cfg).await?,
        Command::Project { command } => {
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
            let mut db = postgres::connect_admin(&cfg).await?;
            match command {
                ContextCommand::Pack(args) => {
                    retrieval::build_context_pack(&cfg, &mut db, &args).await?
                }
            }
        }
        Command::Index { command } => match command {
            IndexCommand::Project(args) => {
                let mut db = postgres::connect_admin(&cfg).await?;
                indexer::index_project(&cfg, &mut db, &args).await?;
            }
        },
    }

    Ok(())
}
