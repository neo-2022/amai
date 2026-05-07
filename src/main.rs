#![recursion_limit = "256"]

mod artifact_cleanup;
mod benchmark_matrix;
mod benchmark_measured_approval;
mod benchmark_promotion;
mod benchmark_statistics;
mod bootstrap;
mod capacity_forecast;
mod chat_question;
mod cli;
mod codex_threads;
mod cold_benchmark;
mod compatibility;
mod config;
mod continuity;
mod dashboard;
mod dashboard_assets;
mod dashboard_format;
mod degradation_proof;
mod deployment;
mod edge_cache;
mod eval_verdict;
mod external_benchmark;
mod external_benchmark_conversion;
mod forgetting;
mod hardware_telemetry;
mod indexer;
mod language;
mod mcp;
mod mcp_errors;
mod mcp_task_matrix;
mod memory_task_matrix;
mod nats;
mod observability_policy;
mod observe;
mod onboarding;
mod postgres;
mod profiles;
mod qdrant;
mod regression_explain;
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

use anyhow::{Context, Result};
use clap::Parser;
use cli::{
    AccessPolicyCommand, AgentCommand, BenchmarkCommand, BootstrapCommand, Cli, Command,
    CompatCommand, ContextCommand, ContinuityCommand, DeploymentCommand, ImportPacketCommand,
    IndexCommand, McpCommand, MemoryCommand, NamespaceCommand, ObserveCommand, ProjectCommand,
    RelationCommand, RoleCommand, SharedAssetCommand, SkillCommand, TeamCommand,
    TransferPolicyCommand, VerifyCommand, WorkspaceCommand,
};
use std::path::Path;
use tracing_subscriber::EnvFilter;
use uuid::Uuid;

fn ensure_object_context<'a>(
    value: &'a mut serde_json::Value,
    label: &str,
) -> Result<&'a mut serde_json::Map<String, serde_json::Value>> {
    value
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("{} must be a JSON object", label))
}

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
                BenchmarkCommand::ExternalMemoryPrepare(args) => {
                    external_benchmark::prepare_external_memory_benchmark(
                        &repo_root,
                        &args.benchmark,
                        &args.dataset,
                        args.source_path.as_deref(),
                        args.download_missing,
                        args.output_dir.as_deref(),
                        args.limit,
                    )
                    .await?
                }
                BenchmarkCommand::ExternalMemoryRun(args) => {
                    let cfg = config::AppConfig::from_env()?;
                    let db = postgres::connect_admin(&cfg).await?;
                    external_benchmark::run_external_memory_benchmark_amai(
                        &cfg,
                        &db,
                        &repo_root,
                        &args.requests,
                        &args.predictions,
                        &args.project,
                        &args.namespace,
                        args.status.as_deref(),
                    )
                    .await?
                }
                BenchmarkCommand::ExternalMemoryScore(args) => {
                    let cfg = config::AppConfig::from_env()?;
                    let db = postgres::connect_admin(&cfg).await?;
                    external_benchmark::score_external_memory_benchmark(
                        &db,
                        &args.cases,
                        &args.predictions,
                        args.output.as_deref(),
                    )
                    .await?
                }
                BenchmarkCommand::ExternalMemoryOfficialJudge(args) => {
                    external_benchmark::run_external_memory_official_judge(
                        &args.cases,
                        &args.predictions,
                        &args.eval_results,
                        args.summary.as_deref(),
                        args.allow_live,
                        &args.api_base_url,
                        &args.api_key_env,
                        &args.model,
                    )
                    .await?
                }
                BenchmarkCommand::ExternalMemoryOfficialScore(args) => {
                    external_benchmark::reconcile_external_memory_official_score(
                        &args.cases,
                        &args.eval_results,
                        args.output.as_deref(),
                    )?
                }
                BenchmarkCommand::ExternalMemorySecretScan(args) => {
                    external_benchmark::scan_external_memory_secret_artifacts(
                        &args.output_dir,
                        &args.secret_env,
                        args.min_secret_len,
                    )?
                }
                BenchmarkCommand::ExternalMemorySchema(args) => {
                    external_benchmark::print_external_memory_schema(
                        &repo_root,
                        args.benchmark.as_deref(),
                        &args.dataset,
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
            ContinuityCommand::ClientBudgetTarget(args) => {
                let cfg = config::AppConfig::from_env()?;
                compatibility::assert_supported(&cfg).await?;
                continuity::client_budget_target(&cfg, &args).await?;
            }
            ContinuityCommand::CompactChat(args) => {
                let cfg = config::AppConfig::from_env()?;
                compatibility::assert_supported(&cfg).await?;
                continuity::compact_chat(&cfg, &args).await?;
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
            BootstrapCommand::AgentPreflight(args) => onboarding::print_agent_preflight(&args)?,
            BootstrapCommand::Install(args) => onboarding::run(&args).await?,
            BootstrapCommand::Onboarding(args) => onboarding::run(&args).await?,
            BootstrapCommand::Reconnect(args) => onboarding::reconnect(&args).await?,
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
                        &args.workspace,
                        &args.visibility_scope,
                        &cfg.default_retrieval_mode,
                    )
                    .await?;
                    println!(
                        "project registered: {} ({}) -> {}",
                        project.code, project.display_name, project.repo_root
                    );
                }
                ProjectCommand::List(args) => {
                    let repo_root = args.repo_root.as_ref().map(|item| item.to_string_lossy());
                    let projects =
                        postgres::list_projects(&db, args.code.as_deref(), repo_root.as_deref())
                            .await?;
                    if args.json {
                        println!("{}", serde_json::to_string(&projects)?);
                    } else {
                        for project in projects {
                            println!(
                                "{} :: {} :: {}",
                                project.code, project.display_name, project.repo_root
                            );
                        }
                    }
                }
            }
        }
        Command::Workspace { command } => {
            let cfg = config::AppConfig::from_env()?;
            let db = postgres::connect_admin(&cfg).await?;
            match command {
                WorkspaceCommand::Ensure(args) => {
                    let workspace = postgres::ensure_workspace(
                        &db,
                        &args.code,
                        &args.display_name,
                        &args.status,
                    )
                    .await?;
                    println!(
                        "workspace ensured: {} :: {} :: {}",
                        workspace.code, workspace.display_name, workspace.status
                    );
                }
                WorkspaceCommand::List(args) => {
                    let workspaces = postgres::list_workspaces(&db, args.code.as_deref()).await?;
                    if args.json {
                        println!("{}", serde_json::to_string(&workspaces)?);
                    } else {
                        for workspace in workspaces {
                            println!(
                                "{} :: {} :: {}",
                                workspace.code, workspace.display_name, workspace.status
                            );
                        }
                    }
                }
            }
        }
        Command::Team { command } => {
            let cfg = config::AppConfig::from_env()?;
            let db = postgres::connect_admin(&cfg).await?;
            match command {
                TeamCommand::Ensure(args) => {
                    let team = postgres::ensure_team(
                        &db,
                        &args.workspace,
                        &args.code,
                        &args.display_name,
                        &args.status,
                    )
                    .await?;
                    println!(
                        "team ensured: {} :: {} :: {} :: {} :: {}",
                        team.workspace_code,
                        team.code,
                        team.display_name,
                        team.status,
                        team.team_id
                    );
                }
                TeamCommand::List(args) => {
                    let teams =
                        postgres::list_teams(&db, args.workspace.as_deref(), args.code.as_deref())
                            .await?;
                    if args.json {
                        println!("{}", serde_json::to_string(&teams)?);
                    } else {
                        for team in teams {
                            println!(
                                "{} :: {} :: {} :: {} :: {}",
                                team.workspace_code,
                                team.code,
                                team.display_name,
                                team.status,
                                team.team_id
                            );
                        }
                    }
                }
            }
        }
        Command::Agent { command } => {
            let cfg = config::AppConfig::from_env()?;
            let db = postgres::connect_admin(&cfg).await?;
            match command {
                AgentCommand::Ensure(args) => {
                    let agent = postgres::ensure_agent(
                        &db,
                        &args.workspace,
                        args.team.as_deref(),
                        args.role.as_deref(),
                        &args.code,
                        &args.display_name,
                        &args.visibility_scope,
                        &args.status,
                    )
                    .await?;
                    println!(
                        "agent ensured: {} :: {} :: {} :: {} :: team={} :: role={} :: {} :: {}",
                        agent.workspace_code,
                        agent.agent_id,
                        agent.code,
                        agent.display_name,
                        agent.team_code.as_deref().unwrap_or("-"),
                        agent.role_code.as_deref().unwrap_or("-"),
                        agent.visibility_scope,
                        agent.status
                    );
                }
                AgentCommand::List(args) => {
                    let agents =
                        postgres::list_agents(&db, args.workspace.as_deref(), args.code.as_deref())
                            .await?;
                    if args.json {
                        println!("{}", serde_json::to_string(&agents)?);
                    } else {
                        for agent in agents {
                            println!(
                                "{} :: {} :: {} :: {} :: team={} :: role={} :: {} :: {}",
                                agent.workspace_code,
                                agent.agent_id,
                                agent.code,
                                agent.display_name,
                                agent.team_code.as_deref().unwrap_or("-"),
                                agent.role_code.as_deref().unwrap_or("-"),
                                agent.visibility_scope,
                                agent.status
                            );
                        }
                    }
                }
            }
        }
        Command::Role { command } => {
            let cfg = config::AppConfig::from_env()?;
            let db = postgres::connect_admin(&cfg).await?;
            match command {
                RoleCommand::Ensure(args) => {
                    let role = postgres::ensure_agent_role(
                        &db,
                        &args.workspace,
                        &args.code,
                        &args.display_name,
                        &args.status,
                    )
                    .await?;
                    println!(
                        "role ensured: {} :: {} :: {} :: {} :: {}",
                        role.workspace_code,
                        role.role_id,
                        role.code,
                        role.display_name,
                        role.status
                    );
                }
                RoleCommand::List(args) => {
                    let roles = postgres::list_agent_roles(
                        &db,
                        args.workspace.as_deref(),
                        args.code.as_deref(),
                    )
                    .await?;
                    if args.json {
                        println!("{}", serde_json::to_string(&roles)?);
                    } else {
                        for role in roles {
                            println!(
                                "{} :: {} :: {} :: {} :: {}",
                                role.workspace_code,
                                role.role_id,
                                role.code,
                                role.display_name,
                                role.status
                            );
                        }
                    }
                }
            }
        }
        Command::AccessPolicy { command } => {
            let cfg = config::AppConfig::from_env()?;
            let db = postgres::connect_admin(&cfg).await?;
            match command {
                AccessPolicyCommand::Ensure(args) => {
                    let policy = postgres::ensure_access_policy(
                        &db,
                        &args.workspace,
                        args.role.as_deref(),
                        args.team.as_deref(),
                        args.project.as_deref(),
                        &args.code,
                        &args.display_name,
                        &args.object_class,
                        &args.scope_type,
                        args.precedence,
                        args.can_read,
                        args.can_write,
                        args.can_link,
                        args.can_import,
                        args.can_promote,
                        args.can_share_further,
                        args.can_archive,
                        args.can_delete,
                        args.can_quarantine,
                        args.can_approve_transfer,
                        args.human_override,
                        args.override_reason.as_deref(),
                        &args.status,
                    )
                    .await?;
                    println!(
                        "access policy ensured: {} :: {} :: {} :: {} :: {} :: {} :: role={} :: team={} :: project={} :: precedence={} :: read={} :: write={} :: link={} :: import={} :: promote={} :: share={} :: archive={} :: delete={} :: quarantine={} :: approve={} :: override={} :: reason={} :: {}",
                        policy.workspace_code,
                        policy.access_policy_id,
                        policy.code,
                        policy.display_name,
                        policy.object_class,
                        policy.scope_type,
                        policy.role_code.as_deref().unwrap_or("-"),
                        policy.team_code.as_deref().unwrap_or("-"),
                        policy.project_code.as_deref().unwrap_or("-"),
                        policy.precedence,
                        policy.can_read,
                        policy.can_write,
                        policy.can_link,
                        policy.can_import,
                        policy.can_promote,
                        policy.can_share_further,
                        policy.can_archive,
                        policy.can_delete,
                        policy.can_quarantine,
                        policy.can_approve_transfer,
                        policy.human_override,
                        policy.override_reason.as_deref().unwrap_or("-"),
                        policy.status
                    );
                }
                AccessPolicyCommand::Get(args) => {
                    let access_policy_id =
                        Uuid::parse_str(&args.access_policy_id).map_err(|error| {
                            anyhow::anyhow!(
                                "invalid access_policy_id {}: {}",
                                args.access_policy_id,
                                error
                            )
                        })?;
                    let policy = postgres::get_access_policy(&db, access_policy_id).await?;
                    println!("{}", serde_json::to_string(&policy)?);
                }
                AccessPolicyCommand::List(args) => {
                    let policies = postgres::list_access_policies(
                        &db,
                        args.workspace.as_deref(),
                        args.code.as_deref(),
                    )
                    .await?;
                    if args.json {
                        println!("{}", serde_json::to_string(&policies)?);
                    } else {
                        for policy in policies {
                            println!(
                                "{} :: {} :: {} :: {} :: role={} :: team={} :: project={} :: {} :: {} :: precedence={} :: read={} :: write={} :: link={} :: import={} :: promote={} :: share={} :: archive={} :: delete={} :: quarantine={} :: approve={} :: override={} :: reason={} :: status={}",
                                policy.workspace_code,
                                policy.access_policy_id,
                                policy.code,
                                policy.display_name,
                                policy.role_code.as_deref().unwrap_or("-"),
                                policy.team_code.as_deref().unwrap_or("-"),
                                policy.project_code.as_deref().unwrap_or("-"),
                                policy.object_class,
                                policy.scope_type,
                                policy.precedence,
                                policy.can_read,
                                policy.can_write,
                                policy.can_link,
                                policy.can_import,
                                policy.can_promote,
                                policy.can_share_further,
                                policy.can_archive,
                                policy.can_delete,
                                policy.can_quarantine,
                                policy.can_approve_transfer,
                                policy.human_override,
                                policy.override_reason.as_deref().unwrap_or("-"),
                                policy.status
                            );
                        }
                    }
                }
            }
        }
        Command::SharedAsset { command } => {
            let cfg = config::AppConfig::from_env()?;
            let db = postgres::connect_admin(&cfg).await?;
            match command {
                SharedAssetCommand::Ensure(args) => {
                    let source_event_ids_json = serde_json::json!(args.source_event_ids);
                    let artifact_refs_json = serde_json::json!(args.artifact_refs);
                    let message_refs_json = serde_json::json!(args.message_refs);
                    let evidence_span = args
                        .evidence_span_json
                        .as_deref()
                        .map(serde_json::from_str::<serde_json::Value>)
                        .transpose()
                        .context("invalid --evidence-span-json for shared asset ensure")?
                        .unwrap_or_else(|| serde_json::json!({}));
                    let asset = postgres::ensure_shared_asset(
                        &db,
                        &args.workspace,
                        &args.code,
                        &args.display_name,
                        &args.asset_kind,
                        args.source_project.as_deref(),
                        args.transfer_policy.as_deref(),
                        &args.visibility_scope,
                        &args.status,
                        args.source_kind.as_deref(),
                        Some(&source_event_ids_json),
                        Some(&artifact_refs_json),
                        Some(&message_refs_json),
                        Some(&evidence_span),
                        Some(&args.derivation_kind),
                        Some(&args.schema_version),
                    )
                    .await?;
                    if args.json {
                        println!("{}", serde_json::to_string(&asset)?);
                    } else {
                        println!(
                            "shared asset ensured: {} :: {} :: {} :: {} :: {} :: source={} :: policy={} :: {} :: {}",
                            asset.workspace_code,
                            asset.shared_asset_id,
                            asset.code,
                            asset.display_name,
                            asset.asset_kind,
                            asset.source_project_code.as_deref().unwrap_or("-"),
                            asset.transfer_policy_code.as_deref().unwrap_or("-"),
                            asset.visibility_scope,
                            asset.status
                        );
                    }
                }
                SharedAssetCommand::Bind(args) => {
                    let source_event_ids_json = serde_json::json!(args.source_event_ids);
                    let artifact_refs_json = serde_json::json!(args.artifact_refs);
                    let message_refs_json = serde_json::json!(args.message_refs);
                    let evidence_span = args
                        .evidence_span_json
                        .as_deref()
                        .map(serde_json::from_str::<serde_json::Value>)
                        .transpose()
                        .context("invalid --evidence-span-json for shared asset bind")?
                        .unwrap_or_else(|| serde_json::json!({}));
                    postgres::bind_shared_asset_to_project(
                        &db,
                        &args.asset,
                        &args.project,
                        &args.binding_kind,
                        args.source_kind.as_deref(),
                        Some(&source_event_ids_json),
                        Some(&artifact_refs_json),
                        Some(&message_refs_json),
                        Some(&evidence_span),
                        Some(&args.derivation_kind),
                        Some(&args.schema_version),
                    )
                    .await?;
                    if args.json {
                        println!(
                            "{}",
                            serde_json::json!({
                                "asset": args.asset,
                                "project": args.project,
                                "binding_kind": args.binding_kind,
                            })
                        );
                    } else {
                        println!(
                            "shared asset bound: {} -> {} :: {}",
                            args.asset, args.project, args.binding_kind
                        );
                    }
                }
                SharedAssetCommand::Get(args) => {
                    let shared_asset_id =
                        Uuid::parse_str(&args.shared_asset_id).map_err(|error| {
                            anyhow::anyhow!(
                                "invalid shared_asset_id {}: {}",
                                args.shared_asset_id,
                                error
                            )
                        })?;
                    let asset = postgres::get_shared_asset(&db, shared_asset_id).await?;
                    println!("{}", serde_json::to_string(&asset)?);
                }
                SharedAssetCommand::List(args) => {
                    let assets = postgres::list_shared_assets(
                        &db,
                        args.workspace.as_deref(),
                        args.project.as_deref(),
                        args.code.as_deref(),
                    )
                    .await?;
                    if args.json {
                        println!("{}", serde_json::to_string(&assets)?);
                    } else {
                        for asset in assets {
                            println!(
                                "{} :: {} :: {} :: {} :: {} :: source={} :: policy={} :: {} :: {}",
                                asset.workspace_code,
                                asset.shared_asset_id,
                                asset.code,
                                asset.display_name,
                                asset.asset_kind,
                                asset.source_project_code.as_deref().unwrap_or("-"),
                                asset.transfer_policy_code.as_deref().unwrap_or("-"),
                                asset.visibility_scope,
                                asset.status
                            );
                        }
                    }
                }
            }
        }
        Command::Skill { command } => {
            let cfg = config::AppConfig::from_env()?;
            let db = postgres::connect_admin(&cfg).await?;
            match command {
                SkillCommand::CreateCandidate(args) => {
                    let mut skill_evidence_span = args
                        .skill_evidence_span_json
                        .as_deref()
                        .map(serde_json::from_str::<serde_json::Value>)
                        .transpose()
                        .context("invalid --evidence-span-json for skill candidate")?
                        .unwrap_or_else(|| serde_json::json!({}));
                    if args.skill_changed_by.is_some() || args.skill_change_reason.is_some() {
                        let change_summary = serde_json::json!({
                            "changed_by": args.skill_changed_by,
                            "change_reason": args.skill_change_reason,
                            "recorded_via": "skill_create_candidate",
                        });
                        match &mut skill_evidence_span {
                            serde_json::Value::Object(map) => {
                                map.insert("skill_change_summary".to_string(), change_summary);
                            }
                            _ => {
                                skill_evidence_span = serde_json::json!({
                                    "skill_change_summary": change_summary
                                });
                            }
                        }
                    }
                    let patch_parent_skill_card_id = args
                        .skill_patch_parent_skill_card_id
                        .as_deref()
                        .map(Uuid::parse_str)
                        .transpose()
                        .map_err(|error| {
                            anyhow::anyhow!(
                                "invalid --patch-parent-skill-card-id {:?}: {}",
                                args.skill_patch_parent_skill_card_id,
                                error
                            )
                        })?;
                    let merge_group_id = args
                        .skill_merge_group_id
                        .as_deref()
                        .map(Uuid::parse_str)
                        .transpose()
                        .map_err(|error| {
                            anyhow::anyhow!(
                                "invalid --merge-group-id {:?}: {}",
                                args.skill_merge_group_id,
                                error
                            )
                        })?;
                    let card = postgres::create_skill_card_candidate_with_refinement(
                        &db,
                        &args.project,
                        &args.namespace,
                        &args.skill_id,
                        args.skill_version,
                        &args.skill_title,
                        &args.skill_goal,
                        &args.skill_trigger_conditions,
                        &args.skill_preconditions,
                        &args.skill_execution_steps,
                        &args.skill_stop_conditions,
                        &args.skill_forbidden_when,
                        args.skill_expected_outcome.as_deref(),
                        &args.skill_scope_type,
                        &args.skill_owner_scope,
                        &args.skill_runtime_constraints,
                        &args.skill_model_constraints,
                        &args.skill_tool_constraints,
                        &args.skill_context_constraints,
                        &args.skill_source_event_ids,
                        &args.skill_artifact_refs,
                        &skill_evidence_span,
                        args.skill_candidate_class.as_deref(),
                        args.skill_refinement_action.as_deref(),
                        patch_parent_skill_card_id,
                        merge_group_id,
                        Some(&args.skill_derivation_kind),
                    )
                    .await?;
                    println!(
                        "skill candidate created: {} :: {} :: {} :: {}@v{} :: {} :: trust={} :: verify={}",
                        card.skill_card_id,
                        card.project_code,
                        card.namespace_code,
                        card.skill_id,
                        card.skill_version,
                        card.skill_title,
                        card.skill_trust_state,
                        card.skill_verification_state
                    );
                }
                SkillCommand::AddEvidence(args) => {
                    let skill_card_id = Uuid::parse_str(&args.skill_card_id).map_err(|error| {
                        anyhow::anyhow!("invalid skill_card_id {}: {}", args.skill_card_id, error)
                    })?;
                    let message_refs_json = serde_json::json!(args.message_refs);
                    let evidence_span = args
                        .evidence_span_json
                        .as_deref()
                        .map(serde_json::from_str::<serde_json::Value>)
                        .transpose()
                        .context("invalid --evidence-span-json for skill evidence")?
                        .unwrap_or_else(|| serde_json::json!({}));
                    let bundle = postgres::create_skill_evidence_bundle(
                        &db,
                        skill_card_id,
                        &args.evidence_kind,
                        args.summary.as_deref(),
                        &args.source_event_ids,
                        &args.artifact_refs,
                        args.source_kind.as_deref(),
                        Some(&message_refs_json),
                        Some(&evidence_span),
                        Some(&args.derivation_kind),
                        Some(&args.schema_version),
                    )
                    .await?;
                    println!(
                        "skill evidence added: {} :: skill={} :: {} :: {}",
                        bundle.skill_evidence_bundle_id,
                        bundle.skill_card_id,
                        bundle.evidence_kind,
                        bundle.summary.as_deref().unwrap_or("-")
                    );
                }
                SkillCommand::GetEvidence(args) => {
                    let skill_evidence_bundle_id = Uuid::parse_str(&args.skill_evidence_bundle_id)
                        .map_err(|error| {
                            anyhow::anyhow!(
                                "invalid skill_evidence_bundle_id {}: {}",
                                args.skill_evidence_bundle_id,
                                error
                            )
                        })?;
                    let bundle =
                        postgres::get_skill_evidence_bundle(&db, skill_evidence_bundle_id).await?;
                    println!("{}", serde_json::to_string(&bundle)?);
                }
                SkillCommand::RecordTriggerMatch(args) => {
                    let skill_card_id = Uuid::parse_str(&args.skill_card_id).map_err(|error| {
                        anyhow::anyhow!("invalid skill_card_id {}: {}", args.skill_card_id, error)
                    })?;
                    let source_event_ids_json = serde_json::json!(args.source_event_ids);
                    let artifact_refs_json = serde_json::json!(args.artifact_refs);
                    let message_refs_json = serde_json::json!(args.message_refs);
                    let evidence_span = args
                        .evidence_span_json
                        .as_deref()
                        .map(serde_json::from_str::<serde_json::Value>)
                        .transpose()
                        .context("invalid --evidence-span-json for skill trigger match")?
                        .unwrap_or_else(|| serde_json::json!({}));
                    let record = postgres::record_skill_trigger_match(
                        &db,
                        skill_card_id,
                        &args.match_scope,
                        &args.trigger_input,
                        args.matched,
                        args.summary.as_deref(),
                        args.source_kind.as_deref(),
                        Some(&source_event_ids_json),
                        Some(&artifact_refs_json),
                        Some(&message_refs_json),
                        Some(&evidence_span),
                        Some(&args.derivation_kind),
                        Some(&args.schema_version),
                    )
                    .await?;
                    println!(
                        "skill trigger recorded: {} :: skill={} :: {} :: matched={} :: {}",
                        record.skill_trigger_match_id,
                        record.skill_card_id,
                        record.match_scope,
                        record.matched,
                        record.summary.as_deref().unwrap_or("-")
                    );
                }
                SkillCommand::GetTriggerMatch(args) => {
                    let skill_trigger_match_id = Uuid::parse_str(&args.skill_trigger_match_id)
                        .map_err(|error| {
                            anyhow::anyhow!(
                                "invalid skill_trigger_match_id {}: {}",
                                args.skill_trigger_match_id,
                                error
                            )
                        })?;
                    let record =
                        postgres::get_skill_trigger_match(&db, skill_trigger_match_id).await?;
                    println!("{}", serde_json::to_string(&record)?);
                }
                SkillCommand::RecordTrialRun(args) => {
                    let skill_card_id = Uuid::parse_str(&args.skill_card_id).map_err(|error| {
                        anyhow::anyhow!("invalid skill_card_id {}: {}", args.skill_card_id, error)
                    })?;
                    let source_event_ids_json = serde_json::json!(args.source_event_ids);
                    let artifact_refs_json = serde_json::json!(args.artifact_refs);
                    let message_refs_json = serde_json::json!(args.message_refs);
                    let mut evidence_span = args
                        .evidence_span_json
                        .as_deref()
                        .map(serde_json::from_str::<serde_json::Value>)
                        .transpose()
                        .context("invalid --evidence-span-json for skill trial run")?
                        .unwrap_or_else(|| serde_json::json!({}));
                    if let Some(context) = args.context.as_deref() {
                        let map = ensure_object_context(
                            &mut evidence_span,
                            "skill trial run evidence span",
                        )?;
                        map.insert("context".to_string(), serde_json::json!(context));
                    }
                    let record = postgres::record_skill_trial_run(
                        &db,
                        skill_card_id,
                        &args.application_mode,
                        args.task_label.as_deref(),
                        args.runtime.as_deref(),
                        args.model.as_deref(),
                        args.tool.as_deref(),
                        args.matched,
                        args.applied,
                        &args.outcome,
                        args.summary.as_deref(),
                        args.source_kind.as_deref(),
                        Some(&source_event_ids_json),
                        Some(&artifact_refs_json),
                        Some(&message_refs_json),
                        Some(&evidence_span),
                        Some(&args.derivation_kind),
                        Some(&args.schema_version),
                    )
                    .await?;
                    println!(
                        "skill trial recorded: {} :: skill={} :: {} :: outcome={} :: matched={} :: applied={}",
                        record.skill_trial_run_id,
                        record.skill_card_id,
                        record.application_mode,
                        record.outcome,
                        record.matched,
                        record.applied
                    );
                }
                SkillCommand::GetTrialRun(args) => {
                    let skill_trial_run_id =
                        Uuid::parse_str(&args.skill_trial_run_id).map_err(|error| {
                            anyhow::anyhow!(
                                "invalid skill_trial_run_id {}: {}",
                                args.skill_trial_run_id,
                                error
                            )
                        })?;
                    let record = postgres::get_skill_trial_run(&db, skill_trial_run_id).await?;
                    println!("{}", serde_json::to_string(&record)?);
                }
                SkillCommand::RecordEval(args) => {
                    let skill_card_id = Uuid::parse_str(&args.skill_card_id).map_err(|error| {
                        anyhow::anyhow!("invalid skill_card_id {}: {}", args.skill_card_id, error)
                    })?;
                    let source_event_ids_json = serde_json::json!(args.source_event_ids);
                    let artifact_refs_json = serde_json::json!(args.artifact_refs);
                    let message_refs_json = serde_json::json!(args.message_refs);
                    let evidence_span = args
                        .evidence_span_json
                        .as_deref()
                        .map(serde_json::from_str::<serde_json::Value>)
                        .transpose()
                        .context("invalid --evidence-span-json for skill eval")?
                        .unwrap_or_else(|| serde_json::json!({}));
                    let eval = postgres::record_skill_eval(
                        &db,
                        skill_card_id,
                        &args.verdict,
                        &args.evaluator_source,
                        args.safe_to_apply,
                        args.quality_ok,
                        args.truth_ok,
                        args.utility_delta,
                        args.summary.as_deref(),
                        args.source_kind.as_deref(),
                        Some(&source_event_ids_json),
                        Some(&artifact_refs_json),
                        Some(&message_refs_json),
                        Some(&evidence_span),
                        Some(&args.derivation_kind),
                        Some(&args.schema_version),
                    )
                    .await?;
                    println!(
                        "skill eval recorded: {} :: skill={} :: verdict={} :: safe={} :: quality={} :: truth={} :: utility_delta={:.3}",
                        eval.skill_eval_id,
                        eval.skill_card_id,
                        eval.verdict,
                        eval.safe_to_apply,
                        eval.quality_ok,
                        eval.truth_ok,
                        eval.utility_delta
                    );
                }
                SkillCommand::GetEval(args) => {
                    let skill_eval_id = Uuid::parse_str(&args.skill_eval_id).map_err(|error| {
                        anyhow::anyhow!("invalid skill_eval_id {}: {}", args.skill_eval_id, error)
                    })?;
                    let record = postgres::get_skill_eval(&db, skill_eval_id).await?;
                    println!("{}", serde_json::to_string(&record)?);
                }
                SkillCommand::RecordReuse(args) => {
                    let skill_card_id = Uuid::parse_str(&args.skill_card_id).map_err(|error| {
                        anyhow::anyhow!("invalid skill_card_id {}: {}", args.skill_card_id, error)
                    })?;
                    let message_refs_json = serde_json::json!(args.message_refs);
                    let mut evidence_span = args
                        .evidence_span_json
                        .as_deref()
                        .map(serde_json::from_str::<serde_json::Value>)
                        .transpose()
                        .context("invalid --evidence-span-json for skill reuse log")?
                        .unwrap_or_else(|| serde_json::json!({}));
                    {
                        let map = ensure_object_context(
                            &mut evidence_span,
                            "skill reuse log evidence span",
                        )?;
                        if let Some(context) = args.context.as_deref() {
                            map.insert("context".to_string(), serde_json::json!(context));
                        }
                        map.insert("matched".to_string(), serde_json::json!(args.matched));
                        map.insert("applied".to_string(), serde_json::json!(args.applied));
                    }
                    let log = postgres::record_skill_reuse_log(
                        &db,
                        skill_card_id,
                        &args.reuse_mode,
                        args.task_label.as_deref(),
                        &args.outcome,
                        args.summary.as_deref(),
                        &args.source_event_ids,
                        &args.artifact_refs,
                        args.source_kind.as_deref(),
                        Some(&message_refs_json),
                        Some(&evidence_span),
                        Some(&args.derivation_kind),
                        Some(&args.schema_version),
                    )
                    .await?;
                    println!(
                        "skill reuse recorded: {} :: skill={} :: {} :: outcome={} :: {}",
                        log.skill_reuse_log_id,
                        log.skill_card_id,
                        log.reuse_mode,
                        log.outcome,
                        log.summary.as_deref().unwrap_or("-")
                    );
                }
                SkillCommand::GetReuse(args) => {
                    let skill_reuse_log_id =
                        Uuid::parse_str(&args.skill_reuse_log_id).map_err(|error| {
                            anyhow::anyhow!(
                                "invalid skill_reuse_log_id {}: {}",
                                args.skill_reuse_log_id,
                                error
                            )
                        })?;
                    let record = postgres::get_skill_reuse_log(&db, skill_reuse_log_id).await?;
                    println!("{}", serde_json::to_string(&record)?);
                }
                SkillCommand::List(args) => {
                    let cards = postgres::list_skill_cards(
                        &db,
                        args.project.as_deref(),
                        args.namespace.as_deref(),
                        args.skill_id.as_deref(),
                    )
                    .await?;
                    if args.json {
                        println!("{}", serde_json::to_string(&cards)?);
                    } else {
                        for card in cards {
                            println!(
                                "{} :: {} :: {} :: {}@v{} :: trust={} :: verify={} :: utility={:.3} :: success={} :: failure={} :: reuse={} :: shadow_pass={} :: shadow_fail={}",
                                card.skill_card_id,
                                card.project_code,
                                card.namespace_code,
                                card.skill_id,
                                card.skill_version,
                                card.skill_trust_state,
                                card.skill_verification_state,
                                card.skill_utility_score,
                                card.skill_success_count,
                                card.skill_failure_count,
                                card.skill_reuse_count,
                                card.skill_shadow_pass_count,
                                card.skill_shadow_fail_count
                            );
                        }
                    }
                }
                SkillCommand::Review(args) => {
                    let skill_card_id = Uuid::parse_str(&args.skill_card_id).map_err(|error| {
                        anyhow::anyhow!("invalid skill_card_id {}: {}", args.skill_card_id, error)
                    })?;
                    let payload = postgres::build_skill_review_payload(&db, skill_card_id).await?;
                    println!("{}", serde_json::to_string_pretty(&payload)?);
                }
                SkillCommand::ExecutionCard(args) => {
                    let payload = postgres::build_skill_execution_cards(
                        &db,
                        &args.project,
                        &args.namespace,
                        args.context.as_deref(),
                        args.runtime.as_deref(),
                        args.model.as_deref(),
                        args.tool.as_deref(),
                        args.allow_trial,
                        args.include_shadow,
                        args.without_amai_but_measuring,
                    )
                    .await?;
                    println!("{}", serde_json::to_string_pretty(&payload)?);
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
                        args.project_link_type.as_deref(),
                        &args.shared_contour,
                        &args.visibility_scope,
                        &args.relation_status,
                        args.requires_approval,
                        args.transfer_policy.as_deref(),
                        &args.access_mode,
                    )
                    .await?;
                    println!(
                        "relation ensured: {} -> {} [{} / {} / {} / {}]",
                        args.source,
                        args.target,
                        args.relation_type,
                        args.shared_contour,
                        args.relation_status,
                        args.access_mode
                    );
                }
                RelationCommand::Update(args) => {
                    postgres::update_relation(
                        &db,
                        postgres::RelationUpdate {
                            source_code: &args.source,
                            target_code: &args.target,
                            relation_type: &args.relation_type,
                            shared_contour: &args.shared_contour,
                            project_link_type: args.project_link_type.as_deref(),
                            visibility_scope: args.visibility_scope.as_deref(),
                            relation_status: args.relation_status.as_deref(),
                            requires_approval: args.requires_approval,
                            transfer_policy_code: args.transfer_policy.as_deref(),
                            access_mode: args.access_mode.as_deref(),
                            actor_agent_code: args.actor_agent.as_deref(),
                            override_reason: args.override_reason.as_deref(),
                        },
                    )
                    .await?;
                    println!(
                        "relation updated: {} -> {} [{} / {}]",
                        args.source, args.target, args.relation_type, args.shared_contour
                    );
                }
            }
        }
        Command::TransferPolicy { command } => {
            let cfg = config::AppConfig::from_env()?;
            let db = postgres::connect_admin(&cfg).await?;
            match command {
                TransferPolicyCommand::Ensure(args) => {
                    let policy = postgres::ensure_transfer_policy(
                        &db,
                        &args.workspace,
                        &args.code,
                        &args.display_name,
                        &args.default_decision,
                        args.allow_cross_project_read,
                        args.allow_import,
                        args.allow_verified_writeback,
                        args.requires_human_approval,
                    )
                    .await?;
                    println!(
                        "transfer policy ensured: {} :: {} :: {} :: {} :: {} :: read={} :: import={} :: verify={} :: approval={}",
                        policy.workspace_code,
                        policy.transfer_policy_id,
                        policy.code,
                        policy.display_name,
                        policy.default_decision,
                        policy.allow_cross_project_read,
                        policy.allow_import,
                        policy.allow_verified_writeback,
                        policy.requires_human_approval
                    );
                }
                TransferPolicyCommand::Get(args) => {
                    let transfer_policy_id =
                        Uuid::parse_str(&args.transfer_policy_id).map_err(|error| {
                            anyhow::anyhow!(
                                "invalid transfer_policy_id {}: {}",
                                args.transfer_policy_id,
                                error
                            )
                        })?;
                    let policy = postgres::get_transfer_policy(&db, transfer_policy_id).await?;
                    println!("{}", serde_json::to_string(&policy)?);
                }
                TransferPolicyCommand::List(args) => {
                    let policies = postgres::list_transfer_policies(
                        &db,
                        args.workspace.as_deref(),
                        args.code.as_deref(),
                    )
                    .await?;
                    if args.json {
                        println!("{}", serde_json::to_string(&policies)?);
                    } else {
                        for policy in policies {
                            println!(
                                "{} :: {} :: {} :: {} :: read={} :: import={} :: verify={} :: approval={}",
                                policy.workspace_code,
                                policy.code,
                                policy.display_name,
                                policy.default_decision,
                                policy.allow_cross_project_read,
                                policy.allow_import,
                                policy.allow_verified_writeback,
                                policy.requires_human_approval
                            );
                        }
                    }
                }
            }
        }
        Command::ImportPacket { command } => {
            let cfg = config::AppConfig::from_env()?;
            let db = postgres::connect_admin(&cfg).await?;
            match command {
                ImportPacketCommand::Create(args) => {
                    let source_event_ids_json = serde_json::json!(args.source_event_ids);
                    let message_refs_json = serde_json::json!(args.message_refs);
                    let evidence_span = args
                        .evidence_span_json
                        .as_deref()
                        .map(serde_json::from_str::<serde_json::Value>)
                        .transpose()
                        .context("invalid --evidence-span-json for import packet")?
                        .unwrap_or_else(|| serde_json::json!({}));
                    let packet = postgres::create_import_packet(
                        &db,
                        &args.source_project,
                        &args.target_project,
                        args.transfer_policy.as_deref(),
                        args.requested_by_agent.as_deref(),
                        &args.status,
                        args.summary.as_deref(),
                        args.reason.as_deref(),
                        &args.imported_by_agent_scope,
                        &args.trust_state,
                        &args.verification_state,
                        &args.borrowed_status,
                        args.can_promote_after_verification,
                        &args.memory_object_ids,
                        &args.artifact_refs,
                        args.source_kind.as_deref(),
                        Some(&source_event_ids_json),
                        Some(&message_refs_json),
                        Some(&evidence_span),
                        Some(&args.derivation_kind),
                        Some(&args.schema_version),
                    )
                    .await?;
                    if args.json {
                        println!("{}", serde_json::to_string(&packet)?);
                    } else {
                        println!(
                            "import packet created: {} :: {} -> {} :: {} :: allowed={} :: scope={} :: borrowed={} :: trust={} :: verify={} :: promote={} :: reason={}",
                            packet.import_packet_id,
                            packet.source_project_code,
                            packet.target_project_code,
                            packet.status,
                            packet.allowed_by_project_link,
                            packet.imported_by_agent_scope,
                            packet.borrowed_status,
                            packet.trust_state,
                            packet.verification_state,
                            packet.can_promote_after_verification,
                            packet.reason.as_deref().unwrap_or("-")
                        );
                    }
                }
                ImportPacketCommand::Get(args) => {
                    let import_packet_id =
                        Uuid::parse_str(&args.import_packet_id).map_err(|error| {
                            anyhow::anyhow!(
                                "invalid import_packet_id {}: {}",
                                args.import_packet_id,
                                error
                            )
                        })?;
                    let packet = postgres::get_import_packet(&db, import_packet_id).await?;
                    println!("{}", serde_json::to_string(&packet)?);
                }
                ImportPacketCommand::Update(args) => {
                    let import_packet_id =
                        Uuid::parse_str(&args.import_packet_id).map_err(|error| {
                            anyhow::anyhow!(
                                "invalid import_packet_id {}: {}",
                                args.import_packet_id,
                                error
                            )
                        })?;
                    let packet = postgres::update_import_packet(
                        &db,
                        postgres::ImportPacketUpdate {
                            import_packet_id,
                            status: args.status.as_deref(),
                            summary: args.summary.as_deref(),
                            reason: args.reason.as_deref(),
                            imported_by_agent_scope: args.imported_by_agent_scope.as_deref(),
                            trust_state: args.trust_state.as_deref(),
                            verification_state: args.verification_state.as_deref(),
                            borrowed_status: args.borrowed_status.as_deref(),
                            can_promote_after_verification: args.can_promote_after_verification,
                            actor_agent_code: args.actor_agent.as_deref(),
                        },
                    )
                    .await?;
                    if args.json {
                        println!("{}", serde_json::to_string(&packet)?);
                    } else {
                        println!(
                            "import packet updated: {} :: {} -> {} :: {} :: allowed={} :: scope={} :: borrowed={} :: trust={} :: verify={} :: promote={} :: reason={}",
                            packet.import_packet_id,
                            packet.source_project_code,
                            packet.target_project_code,
                            packet.status,
                            packet.allowed_by_project_link,
                            packet.imported_by_agent_scope,
                            packet.borrowed_status,
                            packet.trust_state,
                            packet.verification_state,
                            packet.can_promote_after_verification,
                            packet.reason.as_deref().unwrap_or("-")
                        );
                    }
                }
                ImportPacketCommand::List(args) => {
                    let import_packet_id = args
                        .import_packet_id
                        .as_deref()
                        .map(Uuid::parse_str)
                        .transpose()
                        .map_err(|error| anyhow::anyhow!("invalid import_packet_id: {}", error))?;
                    let packets = postgres::list_import_packets(
                        &db,
                        args.project.as_deref(),
                        import_packet_id,
                    )
                    .await?;
                    if args.json {
                        println!("{}", serde_json::to_string(&packets)?);
                    } else {
                        for packet in packets {
                            println!(
                                "{} :: {} -> {} :: {} :: allowed={} :: scope={} :: borrowed={} :: trust={} :: verify={} :: promote={} :: policy={} :: agent={} :: reason={} :: summary={} :: {}",
                                packet.import_packet_id,
                                packet.source_project_code,
                                packet.target_project_code,
                                packet.status,
                                packet.allowed_by_project_link,
                                packet.imported_by_agent_scope,
                                packet.borrowed_status,
                                packet.trust_state,
                                packet.verification_state,
                                packet.can_promote_after_verification,
                                packet.transfer_policy_code.as_deref().unwrap_or("-"),
                                packet.requested_by_agent_code.as_deref().unwrap_or("-"),
                                packet.reason.as_deref().unwrap_or("-"),
                                packet.summary.as_deref().unwrap_or("-"),
                                packet.created_at
                            );
                        }
                    }
                }
                ImportPacketCommand::ReconcileQuarantine(args) => {
                    let summary =
                        postgres::reconcile_import_packet_quarantines(&db, args.apply, args.limit)
                            .await?;
                    if args.json {
                        println!("{}", serde_json::to_string(&summary)?);
                    } else {
                        println!(
                            "import packet quarantine reconcile :: apply={} :: scanned={} :: released={} :: rejected={} :: held={}",
                            summary.apply,
                            summary.scanned,
                            summary.released,
                            summary.rejected,
                            summary.held
                        );
                        for decision in summary.decisions {
                            println!(
                                "{} :: {} -> {} :: {} :: applied={} :: {}",
                                decision.import_packet_id,
                                decision.source_project_code,
                                decision.target_project_code,
                                decision.decision,
                                decision.action_applied,
                                decision.reason
                            );
                        }
                    }
                }
            }
        }
        Command::Memory { command } => {
            let cfg = config::AppConfig::from_env()?;
            let db = postgres::connect_admin(&cfg).await?;
            match command {
                MemoryCommand::CreateItem(args) => {
                    let import_packet_id = args
                        .import_packet_id
                        .as_deref()
                        .map(Uuid::parse_str)
                        .transpose()
                        .map_err(|error| anyhow::anyhow!("invalid import_packet_id: {}", error))?;
                    let superseded_by_memory_item_id = args
                        .superseded_by_memory_item_id
                        .as_deref()
                        .map(Uuid::parse_str)
                        .transpose()
                        .map_err(|error| {
                            anyhow::anyhow!("invalid superseded_by_memory_item_id: {}", error)
                        })?;
                    let evidence_span =
                        serde_json::from_str::<serde_json::Value>(&args.evidence_span_json)
                            .context("invalid --evidence-span-json for memory item")?;
                    let imported_from =
                        serde_json::from_str::<serde_json::Value>(&args.imported_from_json)
                            .context("invalid --imported-from-json for memory item")?;
                    let metadata = serde_json::from_str::<serde_json::Value>(&args.metadata_json)
                        .context("invalid --metadata-json for memory item")?;
                    let item = postgres::create_memory_item(
                        &db,
                        &args.project,
                        &args.namespace,
                        &postgres::MemoryItemInsert {
                            source_project_code: args.source_project.as_deref(),
                            import_packet_id,
                            owner_agent_code: args.owner_agent.as_deref(),
                            item_kind: &args.item_kind,
                            identity_key: args.identity_key.as_deref(),
                            title: &args.title,
                            summary: args.summary.as_deref(),
                            body: args.body.as_deref(),
                            sensitivity_class: args.sensitivity_class.as_deref(),
                            truth_state: args.truth_state.as_deref(),
                            trust_state: args.trust_state.as_deref(),
                            verification_state: args.verification_state.as_deref(),
                            lifecycle_state: args.lifecycle_state.as_deref(),
                            source_event_ids: &args.source_event_ids,
                            artifact_refs: &args.artifact_refs,
                            message_refs: &args.message_refs,
                            evidence_span: &evidence_span,
                            derivation_kind: args.derivation_kind.as_deref(),
                            observed_at_epoch_ms: args.observed_at_epoch_ms,
                            recorded_at_epoch_ms: args.recorded_at_epoch_ms,
                            valid_from_epoch_ms: args.valid_from_epoch_ms,
                            valid_to_epoch_ms: args.valid_to_epoch_ms,
                            last_verified_at_epoch_ms: args.last_verified_at_epoch_ms,
                            object_version: args.object_version,
                            causation_id: args.causation_id.as_deref(),
                            correlation_id: args.correlation_id.as_deref(),
                            utility_score: args.utility_score,
                            freshness_score: args.freshness_score,
                            retention_class: args.retention_class.as_deref(),
                            ttl_epoch_ms: args.ttl_epoch_ms,
                            decay_policy: None,
                            consolidation_status: None,
                            imported_from: Some(&imported_from),
                            schema_version: args.schema_version.as_deref(),
                            superseded_by_memory_item_id,
                            metadata: &metadata,
                        },
                    )
                    .await?;
                    if args.json {
                        println!("{}", serde_json::to_string(&item)?);
                    } else {
                        println!(
                            "memory item created: {} :: {} :: {} :: {} :: {} :: {}",
                            item.memory_item_id,
                            item.project_code,
                            item.namespace_code
                                .clone()
                                .unwrap_or_else(|| "-".to_string()),
                            item.item_kind,
                            item.truth_state,
                            item.derivation_kind
                        );
                    }
                }
                MemoryCommand::GetItem(args) => {
                    let memory_item_id =
                        Uuid::parse_str(&args.memory_item_id).map_err(|error| {
                            anyhow::anyhow!(
                                "invalid memory_item_id {}: {}",
                                args.memory_item_id,
                                error
                            )
                        })?;
                    let payload = postgres::get_memory_envelope(&db, memory_item_id).await?;
                    println!("{}", serde_json::to_string(&payload)?);
                }
                MemoryCommand::UpdateItem(args) => {
                    let memory_item_id =
                        Uuid::parse_str(&args.memory_item_id).map_err(|error| {
                            anyhow::anyhow!(
                                "invalid memory_item_id {}: {}",
                                args.memory_item_id,
                                error
                            )
                        })?;
                    let superseded_by_memory_item_id = args
                        .superseded_by_memory_item_id
                        .as_deref()
                        .map(Uuid::parse_str)
                        .transpose()
                        .map_err(|error| {
                            anyhow::anyhow!("invalid superseded_by_memory_item_id: {}", error)
                        })?;
                    let item = postgres::update_memory_item(
                        &db,
                        &postgres::MemoryItemUpdate {
                            memory_item_id,
                            summary: args.summary.as_deref(),
                            superseded_by_memory_item_id,
                        },
                    )
                    .await?;
                    if args.json {
                        println!("{}", serde_json::to_string(&item)?);
                    } else {
                        println!(
                            "memory item updated: {} :: version={} :: superseded_by={} :: summary={}",
                            item.memory_item_id,
                            item.object_version,
                            item.superseded_by_memory_item_id
                                .map(|value| value.to_string())
                                .unwrap_or_else(|| "-".to_string()),
                            item.summary.as_deref().unwrap_or("-")
                        );
                    }
                }
                MemoryCommand::CreateProvenance(args) => {
                    let memory_item_id = args
                        .memory_item_id
                        .as_deref()
                        .map(Uuid::parse_str)
                        .transpose()
                        .map_err(|error| anyhow::anyhow!("invalid memory_item_id: {}", error))?;
                    let source_snapshot_id = args
                        .source_snapshot_id
                        .as_deref()
                        .map(Uuid::parse_str)
                        .transpose()
                        .map_err(|error| {
                            anyhow::anyhow!("invalid source_snapshot_id: {}", error)
                        })?;
                    let artifact_ref_id = args
                        .artifact_ref_id
                        .as_deref()
                        .map(Uuid::parse_str)
                        .transpose()
                        .map_err(|error| anyhow::anyhow!("invalid artifact_ref_id: {}", error))?;
                    let message_refs_json = serde_json::json!(args.message_refs);
                    let evidence_span =
                        serde_json::from_str::<serde_json::Value>(&args.evidence_span_json)
                            .context("invalid --evidence-span-json for memory provenance")?;
                    let details = serde_json::from_str::<serde_json::Value>(&args.details_json)
                        .context("invalid --details-json for memory provenance")?;
                    let provenance = postgres::create_memory_provenance(
                        &db,
                        &args.project,
                        &args.namespace,
                        &postgres::MemoryProvenanceInsert {
                            memory_item_id,
                            source_kind: &args.source_kind,
                            source_event_id: args.source_event_id.as_deref(),
                            source_snapshot_id,
                            artifact_ref_id,
                            trust_level: args.trust_level.as_deref(),
                            message_refs: Some(&message_refs_json),
                            evidence_span: Some(&evidence_span),
                            derivation_kind: args.derivation_kind.as_deref(),
                            observed_at_epoch_ms: args.observed_at_epoch_ms,
                            recorded_at_epoch_ms: args.recorded_at_epoch_ms,
                            valid_from_epoch_ms: args.valid_from_epoch_ms,
                            valid_to_epoch_ms: args.valid_to_epoch_ms,
                            schema_version: args.schema_version.as_deref(),
                            details: &details,
                        },
                    )
                    .await?;
                    println!(
                        "memory provenance created: {} :: {} :: {} :: {}",
                        provenance.memory_provenance_id,
                        provenance.project_code,
                        provenance.namespace_code.as_deref().unwrap_or("-"),
                        provenance.source_kind
                    );
                }
                MemoryCommand::GetProvenance(args) => {
                    let memory_provenance_id = Uuid::parse_str(&args.memory_provenance_id)
                        .map_err(|error| {
                            anyhow::anyhow!(
                                "invalid memory_provenance_id {}: {}",
                                args.memory_provenance_id,
                                error
                            )
                        })?;
                    let provenance =
                        postgres::get_memory_provenance(&db, memory_provenance_id).await?;
                    println!("{}", serde_json::to_string(&provenance)?);
                }
                MemoryCommand::CreateArtifactRef(args) => {
                    let project = postgres::get_project_by_code(&db, &args.project).await?;
                    let namespace =
                        postgres::find_namespace_by_code(&db, project.project_id, &args.namespace)
                            .await?
                            .ok_or_else(|| {
                                anyhow::anyhow!(
                                    "namespace {} not found for project {}",
                                    args.namespace,
                                    args.project
                                )
                            })?;
                    let source_event_ids_json = serde_json::json!(args.source_event_ids);
                    let message_refs_json = serde_json::json!(args.message_refs);
                    let evidence_span =
                        serde_json::from_str::<serde_json::Value>(&args.evidence_span_json)
                            .context("invalid --evidence-span-json for artifact ref")?;
                    let metadata = serde_json::from_str::<serde_json::Value>(&args.metadata_json)
                        .context("invalid --metadata-json for artifact ref")?;
                    let artifact_ref = postgres::create_artifact_ref(
                        &db,
                        &args.project,
                        &args.namespace,
                        &postgres::ArtifactRefInsert {
                            project_id: project.project_id,
                            namespace_id: namespace.namespace_id,
                            artifact_kind: &args.artifact_kind,
                            bucket: &args.bucket,
                            object_key: &args.object_key,
                            content_type: args.content_type.as_deref(),
                            source_kind: args.source_kind.as_deref(),
                            source_event_ids: Some(&source_event_ids_json),
                            message_refs: Some(&message_refs_json),
                            evidence_span: Some(&evidence_span),
                            derivation_kind: args.derivation_kind.as_deref(),
                            schema_version: args.schema_version.as_deref(),
                            metadata: &metadata,
                        },
                    )
                    .await?;
                    println!(
                        "artifact ref created: {} :: {} :: {} :: {}/{}",
                        artifact_ref.artifact_ref_id,
                        artifact_ref.project_code,
                        artifact_ref.namespace_code,
                        artifact_ref.bucket,
                        artifact_ref.object_key
                    );
                }
                MemoryCommand::GetArtifactRef(args) => {
                    let artifact_ref_id =
                        Uuid::parse_str(&args.artifact_ref_id).map_err(|error| {
                            anyhow::anyhow!(
                                "invalid artifact_ref_id {}: {}",
                                args.artifact_ref_id,
                                error
                            )
                        })?;
                    let artifact_ref = postgres::get_artifact_ref(&db, artifact_ref_id).await?;
                    println!("{}", serde_json::to_string(&artifact_ref)?);
                }
                MemoryCommand::GetLatestRawEvent(args) => {
                    let memory_item_id =
                        Uuid::parse_str(&args.memory_item_id).map_err(|error| {
                            anyhow::anyhow!(
                                "invalid memory_item_id {}: {}",
                                args.memory_item_id,
                                error
                            )
                        })?;
                    let raw_event =
                        postgres::get_latest_memory_raw_event_for_item(&db, memory_item_id).await?;
                    println!("{}", serde_json::to_string(&raw_event)?);
                }
                MemoryCommand::ListWriteOutbox(args) => {
                    let memory_item_id =
                        Uuid::parse_str(&args.memory_item_id).map_err(|error| {
                            anyhow::anyhow!(
                                "invalid memory_item_id {}: {}",
                                args.memory_item_id,
                                error
                            )
                        })?;
                    let outbox_rows =
                        postgres::list_memory_write_outbox_for_item(&db, memory_item_id).await?;
                    println!("{}", serde_json::to_string(&outbox_rows)?);
                }
                MemoryCommand::CreateTaskNode(args) => {
                    let parent_task_node_id = args
                        .parent_task_node_id
                        .as_deref()
                        .map(Uuid::parse_str)
                        .transpose()
                        .map_err(|error| {
                            anyhow::anyhow!("invalid parent_task_node_id: {}", error)
                        })?;
                    let memory_item_id = args
                        .memory_item_id
                        .as_deref()
                        .map(Uuid::parse_str)
                        .transpose()
                        .map_err(|error| anyhow::anyhow!("invalid memory_item_id: {}", error))?;
                    let source_event_ids_json = serde_json::json!(args.source_event_ids);
                    let artifact_refs_json = serde_json::json!(args.artifact_refs);
                    let evidence_span = args
                        .evidence_span_json
                        .as_deref()
                        .map(serde_json::from_str::<serde_json::Value>)
                        .transpose()
                        .context("invalid --evidence-span-json for task node")?
                        .unwrap_or_else(|| serde_json::json!({}));
                    let status_payload =
                        serde_json::from_str::<serde_json::Value>(&args.status_payload_json)
                            .context("invalid --status-payload-json for task node")?;
                    let metadata = serde_json::from_str::<serde_json::Value>(&args.metadata_json)
                        .context("invalid --metadata-json for task node")?;
                    let task_node = postgres::create_task_node(
                        &db,
                        &args.project,
                        &args.namespace,
                        &postgres::TaskNodeInsert {
                            parent_task_node_id,
                            memory_item_id,
                            task_key: args.task_key.as_deref(),
                            task_role: args.task_role.as_deref(),
                            headline: &args.headline,
                            summary: args.summary.as_deref(),
                            next_step: args.next_step.as_deref(),
                            execution_state: args.execution_state.as_deref(),
                            lifecycle_state: args.lifecycle_state.as_deref(),
                            confidence: args.confidence,
                            current_score: args.current_score,
                            reopened_count: args.reopened_count,
                            child_count: args.child_count,
                            closed_child_count: args.closed_child_count,
                            pending_return_count: args.pending_return_count,
                            source_event_ids: Some(&source_event_ids_json),
                            artifact_refs: Some(&artifact_refs_json),
                            evidence_span: Some(&evidence_span),
                            derivation_kind: Some(&args.derivation_kind),
                            status_payload: &status_payload,
                            metadata: &metadata,
                            opened_at_epoch_ms: args.opened_at_epoch_ms,
                            closed_at_epoch_ms: args.closed_at_epoch_ms,
                            archived_at_epoch_ms: args.archived_at_epoch_ms,
                        },
                    )
                    .await?;
                    println!(
                        "task node created: {} :: {} :: {} :: {} :: {} :: {}",
                        task_node.task_node_id,
                        task_node.project_code,
                        task_node
                            .namespace_code
                            .clone()
                            .unwrap_or_else(|| "-".to_string()),
                        task_node.task_role,
                        task_node.execution_state,
                        task_node.derivation_kind
                    );
                }
                MemoryCommand::GetTaskNode(args) => {
                    let task_node_id = Uuid::parse_str(&args.task_node_id).map_err(|error| {
                        anyhow::anyhow!("invalid task_node_id {}: {}", args.task_node_id, error)
                    })?;
                    let task_node = postgres::get_task_node(&db, task_node_id).await?;
                    println!("{}", serde_json::to_string(&task_node)?);
                }
                MemoryCommand::CreateTaskEvent(args) => {
                    let task_node_id = Uuid::parse_str(&args.task_node_id).map_err(|error| {
                        anyhow::anyhow!("invalid task_node_id {}: {}", args.task_node_id, error)
                    })?;
                    let source_snapshot_id = args
                        .source_snapshot_id
                        .as_deref()
                        .map(Uuid::parse_str)
                        .transpose()
                        .map_err(|error| {
                            anyhow::anyhow!("invalid source_snapshot_id: {}", error)
                        })?;
                    let artifact_refs_json = serde_json::json!(args.artifact_refs);
                    let message_refs_json = serde_json::json!(args.message_refs);
                    let evidence_span = args
                        .evidence_span_json
                        .as_deref()
                        .map(serde_json::from_str::<serde_json::Value>)
                        .transpose()
                        .context("invalid --evidence-span-json for task event")?
                        .unwrap_or_else(|| serde_json::json!({}));
                    let event_payload =
                        serde_json::from_str::<serde_json::Value>(&args.event_payload_json)
                            .context("invalid --event-payload-json for task event")?;
                    let task_event = postgres::create_task_event(
                        &db,
                        &args.project,
                        &args.namespace,
                        &postgres::TaskEventInsert {
                            task_node_id,
                            source_snapshot_id,
                            source_event_id: args.source_event_id.as_deref(),
                            event_kind: &args.event_kind,
                            prior_execution_state: args.prior_execution_state.as_deref(),
                            next_execution_state: args.next_execution_state.as_deref(),
                            prior_lifecycle_state: args.prior_lifecycle_state.as_deref(),
                            next_lifecycle_state: args.next_lifecycle_state.as_deref(),
                            source_kind: args.source_kind.as_deref(),
                            artifact_refs: Some(&artifact_refs_json),
                            message_refs: Some(&message_refs_json),
                            evidence_span: Some(&evidence_span),
                            derivation_kind: Some(&args.derivation_kind),
                            schema_version: Some(&args.schema_version),
                            event_payload: &event_payload,
                            recorded_at_epoch_ms: args.recorded_at_epoch_ms,
                        },
                    )
                    .await?;
                    println!(
                        "task event created: {} :: {} :: {} :: {} :: {}",
                        task_event.task_event_id,
                        task_event.project_code,
                        task_event
                            .namespace_code
                            .clone()
                            .unwrap_or_else(|| "-".to_string()),
                        task_event.event_kind,
                        task_event.derivation_kind
                    );
                }
                MemoryCommand::GetTaskEvent(args) => {
                    let task_event_id = Uuid::parse_str(&args.task_event_id).map_err(|error| {
                        anyhow::anyhow!("invalid task_event_id {}: {}", args.task_event_id, error)
                    })?;
                    let task_event = postgres::get_task_event(&db, task_event_id).await?;
                    println!("{}", serde_json::to_string(&task_event)?);
                }
                MemoryCommand::CreateCard(args) => {
                    let provenance =
                        serde_json::from_str::<serde_json::Value>(&args.provenance_json)
                            .context("invalid --provenance-json for memory card")?;
                    let card = postgres::create_memory_card(
                        &db,
                        &args.project,
                        &args.namespace,
                        &args.title,
                        &args.summary,
                        &args.body,
                        &args.tag,
                        &provenance,
                        args.fact_subject.as_deref(),
                        args.fact_predicate.as_deref(),
                        args.fact_object.as_deref(),
                        args.truth_state.as_deref(),
                        args.verification_state.as_deref(),
                        args.status.as_deref(),
                        args.observed_at_epoch_ms,
                        args.recorded_at_epoch_ms,
                        args.valid_from_epoch_ms,
                        args.valid_to_epoch_ms,
                        args.last_verified_at_epoch_ms,
                    )
                    .await?;
                    println!(
                        "memory card created: {} :: {} :: {} :: {} :: {}",
                        card.memory_card_id,
                        card.project_code,
                        card.namespace_code,
                        card.candidate_class,
                        card.derivation_kind
                    );
                }
                MemoryCommand::GetCard(args) => {
                    let memory_card_id =
                        Uuid::parse_str(&args.memory_card_id).map_err(|error| {
                            anyhow::anyhow!(
                                "invalid memory_card_id {}: {}",
                                args.memory_card_id,
                                error
                            )
                        })?;
                    let card = postgres::get_memory_card(&db, memory_card_id).await?;
                    println!("{}", serde_json::to_string(&card)?);
                }
                MemoryCommand::ListCards(args) => {
                    let cards = postgres::list_memory_cards(
                        &db,
                        args.project.as_deref(),
                        args.namespace.as_deref(),
                        args.truth_state.as_deref(),
                        args.status.as_deref(),
                    )
                    .await?;
                    println!("{}", serde_json::to_string(&cards)?);
                }
                MemoryCommand::SupersedeCard(args) => {
                    let memory_card_id =
                        Uuid::parse_str(&args.memory_card_id).map_err(|error| {
                            anyhow::anyhow!(
                                "invalid memory_card_id {}: {}",
                                args.memory_card_id,
                                error
                            )
                        })?;
                    let superseded_by = Uuid::parse_str(&args.superseded_by).map_err(|error| {
                        anyhow::anyhow!("invalid superseded_by {}: {}", args.superseded_by, error)
                    })?;
                    postgres::supersede_memory_card(
                        &db,
                        memory_card_id,
                        superseded_by,
                        args.valid_to_epoch_ms,
                        args.last_verified_at_epoch_ms,
                    )
                    .await?;
                    println!(
                        "memory card superseded: {} -> {}",
                        memory_card_id, superseded_by
                    );
                }
                MemoryCommand::UpdateCardTruthState(args) => {
                    let memory_card_id =
                        Uuid::parse_str(&args.memory_card_id).map_err(|error| {
                            anyhow::anyhow!(
                                "invalid memory_card_id {}: {}",
                                args.memory_card_id,
                                error
                            )
                        })?;
                    postgres::update_memory_card_truth_state(
                        &db,
                        memory_card_id,
                        args.truth_state.as_deref(),
                        args.verification_state.as_deref(),
                        args.status.as_deref(),
                        args.last_verified_at_epoch_ms,
                    )
                    .await?;
                    println!("memory card truth state updated: {}", memory_card_id);
                }
                MemoryCommand::ApplyCardUpdate(args) => {
                    let provenance =
                        serde_json::from_str::<serde_json::Value>(&args.provenance_json)
                            .context("invalid --provenance-json for memory card update")?;
                    let card = postgres::apply_memory_card_update(
                        &db,
                        &args.project,
                        &args.namespace,
                        &args.title,
                        &args.summary,
                        &args.body,
                        &args.tag,
                        &provenance,
                        args.fact_subject.as_deref(),
                        args.fact_predicate.as_deref(),
                        args.fact_object.as_deref(),
                        args.truth_state.as_deref(),
                        args.verification_state.as_deref(),
                        args.status.as_deref(),
                        args.observed_at_epoch_ms,
                        args.recorded_at_epoch_ms,
                        args.valid_from_epoch_ms,
                        args.valid_to_epoch_ms,
                        args.last_verified_at_epoch_ms,
                    )
                    .await?;
                    println!(
                        "memory card updated: {} :: {} :: {} :: {} :: {}",
                        card.memory_card_id,
                        card.project_code,
                        card.namespace_code,
                        card.candidate_class,
                        card.derivation_kind
                    );
                }
                MemoryCommand::CreateEdge(args) => {
                    let source_memory_item_id = Uuid::parse_str(&args.source_memory_item_id)
                        .map_err(|error| {
                            anyhow::anyhow!(
                                "invalid source_memory_item_id {}: {}",
                                args.source_memory_item_id,
                                error
                            )
                        })?;
                    let target_memory_item_id = Uuid::parse_str(&args.target_memory_item_id)
                        .map_err(|error| {
                            anyhow::anyhow!(
                                "invalid target_memory_item_id {}: {}",
                                args.target_memory_item_id,
                                error
                            )
                        })?;
                    let evidence = serde_json::from_str::<serde_json::Value>(&args.evidence_json)
                        .context("invalid --evidence-json for memory edge")?;
                    let source_event_ids_json = serde_json::json!(args.source_event_ids);
                    let artifact_refs_json = serde_json::json!(args.artifact_refs);
                    let message_refs_json = serde_json::json!(args.message_refs);
                    let evidence_span = args
                        .evidence_span_json
                        .as_deref()
                        .map(serde_json::from_str::<serde_json::Value>)
                        .transpose()
                        .context("invalid --evidence-span-json for memory edge")?
                        .unwrap_or_else(|| serde_json::json!({}));
                    let edge = postgres::create_memory_edge(
                        &db,
                        &args.project,
                        &args.namespace,
                        &postgres::MemoryEdgeInsert {
                            source_memory_item_id,
                            target_memory_item_id,
                            edge_kind: &args.edge_kind,
                            edge_state: Some(&args.edge_state),
                            trust_state: args.trust_state.as_deref(),
                            validity_basis: args.validity_basis.as_deref(),
                            score: args.score,
                            evidence: &evidence,
                            source_kind: args.source_kind.as_deref(),
                            source_event_ids: Some(&source_event_ids_json),
                            artifact_refs: Some(&artifact_refs_json),
                            message_refs: Some(&message_refs_json),
                            evidence_span: Some(&evidence_span),
                            derivation_kind: Some(&args.derivation_kind),
                            schema_version: Some(&args.schema_version),
                            valid_from_epoch_ms: args.valid_from_epoch_ms,
                            valid_to_epoch_ms: args.valid_to_epoch_ms,
                        },
                    )
                    .await?;
                    println!(
                        "memory edge created: {} :: {} :: {} -> {} :: {} :: {} :: {}",
                        edge.memory_edge_id,
                        edge.project_code,
                        edge.source_memory_item_id,
                        edge.target_memory_item_id,
                        edge.edge_kind,
                        edge.edge_state,
                        edge.trust_state
                    );
                }
                MemoryCommand::GetEdge(args) => {
                    let memory_edge_id =
                        Uuid::parse_str(&args.memory_edge_id).map_err(|error| {
                            anyhow::anyhow!(
                                "invalid memory_edge_id {}: {}",
                                args.memory_edge_id,
                                error
                            )
                        })?;
                    let edge = postgres::get_memory_edge(&db, memory_edge_id).await?;
                    println!("{}", serde_json::to_string(&edge)?);
                }
                MemoryCommand::CreateConflict(args) => {
                    let left_memory_item_id = args
                        .left_memory_item_id
                        .as_deref()
                        .map(Uuid::parse_str)
                        .transpose()
                        .map_err(|error| {
                            anyhow::anyhow!("invalid left_memory_item_id: {}", error)
                        })?;
                    let right_memory_item_id = args
                        .right_memory_item_id
                        .as_deref()
                        .map(Uuid::parse_str)
                        .transpose()
                        .map_err(|error| {
                            anyhow::anyhow!("invalid right_memory_item_id: {}", error)
                        })?;
                    let evidence = serde_json::from_str::<serde_json::Value>(&args.evidence_json)
                        .context("invalid --evidence-json for memory conflict")?;
                    let source_event_ids_json = serde_json::json!(args.source_event_ids);
                    let artifact_refs_json = serde_json::json!(args.artifact_refs);
                    let message_refs_json = serde_json::json!(args.message_refs);
                    let evidence_span = args
                        .evidence_span_json
                        .as_deref()
                        .map(serde_json::from_str::<serde_json::Value>)
                        .transpose()
                        .context("invalid --evidence-span-json for memory conflict")?
                        .unwrap_or_else(|| serde_json::json!({}));
                    let resolution = args
                        .resolution_json
                        .as_deref()
                        .map(serde_json::from_str::<serde_json::Value>)
                        .transpose()
                        .context("invalid --resolution-json for memory conflict")?;
                    let conflict = postgres::create_memory_conflict(
                        &db,
                        &args.project,
                        &args.namespace,
                        &postgres::MemoryConflictInsert {
                            left_memory_item_id,
                            right_memory_item_id,
                            conflict_kind: &args.conflict_kind,
                            conflict_state: Some(&args.conflict_state),
                            severity: Some(&args.severity),
                            summary: &args.summary,
                            evidence: &evidence,
                            source_kind: args.source_kind.as_deref(),
                            source_event_ids: Some(&source_event_ids_json),
                            artifact_refs: Some(&artifact_refs_json),
                            message_refs: Some(&message_refs_json),
                            evidence_span: Some(&evidence_span),
                            derivation_kind: Some(&args.derivation_kind),
                            schema_version: Some(&args.schema_version),
                            resolution: resolution.as_ref(),
                            detected_at_epoch_ms: args.detected_at_epoch_ms,
                            resolved_at_epoch_ms: args.resolved_at_epoch_ms,
                        },
                    )
                    .await?;
                    println!(
                        "memory conflict created: {} :: {} :: left={} :: right={} :: {} :: {} :: {}",
                        conflict.memory_conflict_id,
                        conflict.project_code,
                        conflict
                            .left_memory_item_id
                            .map(|value| value.to_string())
                            .unwrap_or_else(|| "-".to_string()),
                        conflict
                            .right_memory_item_id
                            .map(|value| value.to_string())
                            .unwrap_or_else(|| "-".to_string()),
                        conflict.conflict_kind,
                        conflict.conflict_state,
                        conflict.severity
                    );
                }
                MemoryCommand::GetConflict(args) => {
                    let memory_conflict_id =
                        Uuid::parse_str(&args.memory_conflict_id).map_err(|error| {
                            anyhow::anyhow!(
                                "invalid memory_conflict_id {}: {}",
                                args.memory_conflict_id,
                                error
                            )
                        })?;
                    let conflict = postgres::get_memory_conflict(&db, memory_conflict_id).await?;
                    println!("{}", serde_json::to_string(&conflict)?);
                }
                MemoryCommand::CreateLinkDecision(args) => {
                    let task_node_id = args
                        .task_node_id
                        .as_deref()
                        .map(Uuid::parse_str)
                        .transpose()
                        .map_err(|error| anyhow::anyhow!("invalid task_node_id: {}", error))?;
                    let retrieval_trace_id = args
                        .retrieval_trace_id
                        .as_deref()
                        .map(Uuid::parse_str)
                        .transpose()
                        .map_err(|error| {
                            anyhow::anyhow!("invalid retrieval_trace_id: {}", error)
                        })?;
                    let candidate_task_node_id = args
                        .candidate_task_node_id
                        .as_deref()
                        .map(Uuid::parse_str)
                        .transpose()
                        .map_err(|error| {
                            anyhow::anyhow!("invalid candidate_task_node_id: {}", error)
                        })?;
                    let decision_payload =
                        serde_json::from_str::<serde_json::Value>(&args.decision_payload_json)
                            .context("invalid --decision-payload-json for memory link decision")?;
                    let source_event_ids_json = serde_json::json!(args.source_event_ids);
                    let artifact_refs_json = serde_json::json!(args.artifact_refs);
                    let message_refs_json = serde_json::json!(args.message_refs);
                    let evidence_span = args
                        .evidence_span_json
                        .as_deref()
                        .map(serde_json::from_str::<serde_json::Value>)
                        .transpose()
                        .context("invalid --evidence-span-json for memory link decision")?
                        .unwrap_or_else(|| serde_json::json!({}));
                    let decision = postgres::create_memory_link_decision(
                        &db,
                        &args.project,
                        &args.namespace,
                        &postgres::MemoryLinkDecisionInsert {
                            task_node_id,
                            retrieval_trace_id,
                            candidate_task_node_id,
                            decision_outcome: &args.decision_outcome,
                            legality_passed: args.legality_passed,
                            scope_filter_passed: args.scope_filter_passed,
                            evidence_sufficient: args.evidence_sufficient,
                            classifier_label: args.classifier_label.as_deref(),
                            classifier_score: args.classifier_score,
                            decision_reason: args.decision_reason.as_deref(),
                            decision_payload: &decision_payload,
                            source_event_ids: Some(&source_event_ids_json),
                            artifact_refs: Some(&artifact_refs_json),
                            message_refs: Some(&message_refs_json),
                            evidence_span: Some(&evidence_span),
                            derivation_kind: Some(&args.derivation_kind),
                            schema_version: Some(&args.schema_version),
                            recorded_at_epoch_ms: args.recorded_at_epoch_ms,
                        },
                    )
                    .await?;
                    println!(
                        "memory link decision created: {} :: {} :: outcome={} :: legality={} :: scope={} :: sufficient={}",
                        decision.memory_link_decision_id,
                        decision.project_code,
                        decision.decision_outcome,
                        decision.legality_passed,
                        decision.scope_filter_passed,
                        decision.evidence_sufficient
                    );
                }
                MemoryCommand::GetLinkDecision(args) => {
                    let memory_link_decision_id = Uuid::parse_str(&args.memory_link_decision_id)
                        .map_err(|error| {
                            anyhow::anyhow!(
                                "invalid memory_link_decision_id {}: {}",
                                args.memory_link_decision_id,
                                error
                            )
                        })?;
                    let decision =
                        postgres::get_memory_link_decision(&db, memory_link_decision_id).await?;
                    println!("{}", serde_json::to_string(&decision)?);
                }
                MemoryCommand::CreatePendingLinkProposal(args) => {
                    let task_node_id = args
                        .task_node_id
                        .as_deref()
                        .map(Uuid::parse_str)
                        .transpose()
                        .map_err(|error| anyhow::anyhow!("invalid task_node_id: {}", error))?;
                    let retrieval_trace_id = args
                        .retrieval_trace_id
                        .as_deref()
                        .map(Uuid::parse_str)
                        .transpose()
                        .map_err(|error| {
                            anyhow::anyhow!("invalid retrieval_trace_id: {}", error)
                        })?;
                    let candidate_task_node_id = args
                        .candidate_task_node_id
                        .as_deref()
                        .map(Uuid::parse_str)
                        .transpose()
                        .map_err(|error| {
                            anyhow::anyhow!("invalid candidate_task_node_id: {}", error)
                        })?;
                    let evidence_payload =
                        serde_json::from_str::<serde_json::Value>(&args.evidence_payload_json)
                            .context("invalid --evidence-payload-json for pending link proposal")?;
                    let source_event_ids_json = serde_json::json!(args.source_event_ids);
                    let artifact_refs_json = serde_json::json!(args.artifact_refs);
                    let message_refs_json = serde_json::json!(args.message_refs);
                    let evidence_span = args
                        .evidence_span_json
                        .as_deref()
                        .map(serde_json::from_str::<serde_json::Value>)
                        .transpose()
                        .context("invalid --evidence-span-json for pending link proposal")?
                        .unwrap_or_else(|| serde_json::json!({}));
                    let proposal = postgres::create_pending_link_proposal(
                        &db,
                        &args.project,
                        &args.namespace,
                        &postgres::PendingLinkProposalInsert {
                            task_node_id,
                            retrieval_trace_id,
                            candidate_task_node_id,
                            proposal_state: Some(&args.proposal_state),
                            proposal_reason: &args.proposal_reason,
                            evidence_request: args.evidence_request.as_deref(),
                            evidence_payload: &evidence_payload,
                            classifier_score: args.classifier_score,
                            ttl_epoch_ms: args.ttl_epoch_ms,
                            source_event_ids: Some(&source_event_ids_json),
                            artifact_refs: Some(&artifact_refs_json),
                            message_refs: Some(&message_refs_json),
                            evidence_span: Some(&evidence_span),
                            derivation_kind: Some(&args.derivation_kind),
                            schema_version: Some(&args.schema_version),
                        },
                    )
                    .await?;
                    println!(
                        "pending link proposal created: {} :: {} :: state={} :: reason={} :: ttl={}",
                        proposal.pending_link_proposal_id,
                        proposal.project_code,
                        proposal.proposal_state,
                        proposal.proposal_reason,
                        proposal
                            .ttl_epoch_ms
                            .map(|value| value.to_string())
                            .unwrap_or_else(|| "-".to_string())
                    );
                }
                MemoryCommand::GetPendingLinkProposal(args) => {
                    let pending_link_proposal_id = Uuid::parse_str(&args.pending_link_proposal_id)
                        .map_err(|error| {
                            anyhow::anyhow!(
                                "invalid pending_link_proposal_id {}: {}",
                                args.pending_link_proposal_id,
                                error
                            )
                        })?;
                    let proposal =
                        postgres::get_pending_link_proposal(&db, pending_link_proposal_id).await?;
                    println!("{}", serde_json::to_string(&proposal)?);
                }
                MemoryCommand::CreateRelationEdge(args) => {
                    let source_memory_card_id = Uuid::parse_str(&args.source_memory_card_id)
                        .map_err(|error| {
                            anyhow::anyhow!(
                                "invalid source_memory_card_id {}: {}",
                                args.source_memory_card_id,
                                error
                            )
                        })?;
                    let target_memory_card_id = Uuid::parse_str(&args.target_memory_card_id)
                        .map_err(|error| {
                            anyhow::anyhow!(
                                "invalid target_memory_card_id {}: {}",
                                args.target_memory_card_id,
                                error
                            )
                        })?;
                    let evidence = serde_json::from_str::<serde_json::Value>(&args.evidence_json)
                        .context("invalid --evidence-json for memory relation edge")?;
                    let source_event_ids_json = serde_json::json!(args.source_event_ids);
                    let artifact_refs_json = serde_json::json!(args.artifact_refs);
                    let message_refs_json = serde_json::json!(args.message_refs);
                    let evidence_span = args
                        .evidence_span_json
                        .as_deref()
                        .map(serde_json::from_str::<serde_json::Value>)
                        .transpose()
                        .context("invalid --evidence-span-json for memory relation edge")?
                        .unwrap_or_else(|| serde_json::json!({}));
                    let relation = postgres::create_memory_relation_edge(
                        &db,
                        &args.project,
                        &args.namespace,
                        source_memory_card_id,
                        target_memory_card_id,
                        &args.relation_type,
                        Some(&args.relation_state),
                        &evidence,
                        args.source_kind.as_deref(),
                        Some(&source_event_ids_json),
                        Some(&artifact_refs_json),
                        Some(&message_refs_json),
                        Some(&evidence_span),
                        Some(&args.derivation_kind),
                        Some(&args.schema_version),
                        args.recorded_at_epoch_ms,
                        args.valid_from_epoch_ms,
                        args.valid_to_epoch_ms,
                    )
                    .await?;
                    println!(
                        "memory relation edge created: {} :: {} :: {} -> {} :: {} :: {}",
                        relation.memory_relation_edge_id,
                        relation.project_code,
                        relation.source_memory_card_id,
                        relation.target_memory_card_id,
                        relation.relation_type,
                        relation.relation_state
                    );
                }
                MemoryCommand::GetRelationEdge(args) => {
                    let memory_relation_edge_id = Uuid::parse_str(&args.memory_relation_edge_id)
                        .map_err(|error| {
                            anyhow::anyhow!(
                                "invalid memory_relation_edge_id {}: {}",
                                args.memory_relation_edge_id,
                                error
                            )
                        })?;
                    let relation =
                        postgres::get_memory_relation_edge(&db, memory_relation_edge_id).await?;
                    println!("{}", serde_json::to_string(&relation)?);
                }
                MemoryCommand::ListRelationEdges(args) => {
                    let project = postgres::get_project_by_code(&db, &args.project).await?;
                    let namespace =
                        postgres::get_namespace_by_code(&db, project.project_id, &args.namespace)
                            .await?;
                    let memory_card_ids = args
                        .memory_card_ids
                        .iter()
                        .map(|item| {
                            Uuid::parse_str(item).map_err(|error| {
                                anyhow::anyhow!("invalid memory_card_id {}: {}", item, error)
                            })
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    let relations = postgres::list_memory_relation_edges_for_cards(
                        &db,
                        project.project_id,
                        namespace.namespace_id,
                        &memory_card_ids,
                        args.at_epoch_ms,
                        args.limit,
                    )
                    .await?;
                    println!("{}", serde_json::to_string(&relations)?);
                }
                MemoryCommand::CreateRestorePack(args) => {
                    let source_snapshot_id = args
                        .source_snapshot_id
                        .as_deref()
                        .map(Uuid::parse_str)
                        .transpose()
                        .map_err(|error| {
                            anyhow::anyhow!("invalid source_snapshot_id: {}", error)
                        })?;
                    let source_event_ids_json = serde_json::json!(args.source_event_ids);
                    let artifact_refs_json = serde_json::json!(args.artifact_refs);
                    let message_refs_json = serde_json::json!(args.message_refs);
                    let evidence_span =
                        serde_json::from_str::<serde_json::Value>(&args.evidence_span_json)
                            .context("invalid --evidence-span-json for restore pack")?;
                    let payload = serde_json::from_str::<serde_json::Value>(&args.payload_json)
                        .context("invalid --payload-json for restore pack")?;
                    let pack = postgres::create_restore_pack(
                        &db,
                        &args.project,
                        &args.namespace,
                        &postgres::RestorePackInsert {
                            agent_scope: args.agent_scope.as_deref(),
                            session_id: args.session_id.as_deref(),
                            thread_id: args.thread_id.as_deref(),
                            source_snapshot_id,
                            source_snapshot_hint: None,
                            pack_kind: &args.pack_kind,
                            source_kind: args.source_kind.as_deref(),
                            source_event_ids: Some(&source_event_ids_json),
                            artifact_refs: Some(&artifact_refs_json),
                            message_refs: Some(&message_refs_json),
                            evidence_span: Some(&evidence_span),
                            derivation_kind: args.derivation_kind.as_deref(),
                            schema_version: args.schema_version.as_deref(),
                            headline: args.headline.as_deref(),
                            summary: args.summary.as_deref(),
                            payload: &payload,
                            captured_at_epoch_ms: args.captured_at_epoch_ms,
                        },
                    )
                    .await?;
                    println!(
                        "restore pack created: {} :: {} :: {} :: {}",
                        pack.restore_pack_id,
                        pack.project_code,
                        pack.namespace_code.as_deref().unwrap_or("-"),
                        pack.pack_kind
                    );
                }
                MemoryCommand::GetRestorePack(args) => {
                    let restore_pack_id =
                        Uuid::parse_str(&args.restore_pack_id).map_err(|error| {
                            anyhow::anyhow!(
                                "invalid restore_pack_id {}: {}",
                                args.restore_pack_id,
                                error
                            )
                        })?;
                    let pack = postgres::get_restore_pack(&db, restore_pack_id).await?;
                    println!("{}", serde_json::to_string(&pack)?);
                }
                MemoryCommand::CreatePolicyRule(args) => {
                    let source_event_ids_json = serde_json::json!(args.source_event_ids);
                    let artifact_refs_json = serde_json::json!(args.artifact_refs);
                    let message_refs_json = serde_json::json!(args.message_refs);
                    let evidence_span =
                        serde_json::from_str::<serde_json::Value>(&args.evidence_span_json)
                            .context("invalid --evidence-span-json for policy rule")?;
                    let rule_payload =
                        serde_json::from_str::<serde_json::Value>(&args.rule_payload_json)
                            .context("invalid --rule-payload-json for policy rule")?;
                    let rule = postgres::create_policy_rule(
                        &db,
                        &args.workspace,
                        &postgres::PolicyRuleInsert {
                            project_code: args.project.as_deref(),
                            namespace_code: args.namespace.as_deref(),
                            rule_code: &args.rule_code,
                            rule_scope: &args.rule_scope,
                            rule_kind: &args.rule_kind,
                            rule_status: args.rule_status.as_deref(),
                            precedence: args.precedence,
                            source_kind: args.source_kind.as_deref(),
                            source_event_ids: Some(&source_event_ids_json),
                            artifact_refs: Some(&artifact_refs_json),
                            message_refs: Some(&message_refs_json),
                            evidence_span: Some(&evidence_span),
                            derivation_kind: args.derivation_kind.as_deref(),
                            schema_version: args.schema_version.as_deref(),
                            rule_payload: &rule_payload,
                        },
                    )
                    .await?;
                    println!(
                        "policy rule created: {} :: {} :: {} :: {}",
                        rule.policy_rule_id, rule.workspace_code, rule.rule_code, rule.rule_status
                    );
                }
                MemoryCommand::GetPolicyRule(args) => {
                    let policy_rule_id =
                        Uuid::parse_str(&args.policy_rule_id).map_err(|error| {
                            anyhow::anyhow!(
                                "invalid policy_rule_id {}: {}",
                                args.policy_rule_id,
                                error
                            )
                        })?;
                    let rule = postgres::get_policy_rule(&db, policy_rule_id).await?;
                    println!("{}", serde_json::to_string(&rule)?);
                }
                MemoryCommand::CreateQuarantineItem(args) => {
                    let entity_id = args
                        .entity_id
                        .as_deref()
                        .map(Uuid::parse_str)
                        .transpose()
                        .map_err(|error| anyhow::anyhow!("invalid entity_id: {}", error))?;
                    let evidence = serde_json::from_str::<serde_json::Value>(&args.evidence_json)
                        .context("invalid --evidence-json for quarantine item")?;
                    let source_event_ids_json = serde_json::json!(args.source_event_ids);
                    let artifact_refs_json = serde_json::json!(args.artifact_refs);
                    let message_refs_json = serde_json::json!(args.message_refs);
                    let evidence_span =
                        serde_json::from_str::<serde_json::Value>(&args.evidence_span_json)
                            .context("invalid --evidence-span-json for quarantine item")?;
                    let item = postgres::create_quarantine_item(
                        &db,
                        &args.workspace,
                        &postgres::QuarantineItemInsert {
                            project_code: args.project.as_deref(),
                            namespace_code: args.namespace.as_deref(),
                            entity_kind: &args.entity_kind,
                            entity_id,
                            quarantine_reason: &args.quarantine_reason,
                            quarantine_state: args.quarantine_state.as_deref(),
                            evidence: &evidence,
                            source_kind: args.source_kind.as_deref(),
                            source_event_ids: Some(&source_event_ids_json),
                            artifact_refs: Some(&artifact_refs_json),
                            message_refs: Some(&message_refs_json),
                            evidence_span: Some(&evidence_span),
                            derivation_kind: args.derivation_kind.as_deref(),
                            schema_version: args.schema_version.as_deref(),
                            quarantined_at_epoch_ms: args.quarantined_at_epoch_ms,
                            released_at_epoch_ms: args.released_at_epoch_ms,
                        },
                    )
                    .await?;
                    println!(
                        "quarantine item created: {} :: {} :: {} :: {}",
                        item.quarantine_item_id,
                        item.workspace_code,
                        item.entity_kind,
                        item.quarantine_state
                    );
                }
                MemoryCommand::GetQuarantineItem(args) => {
                    let quarantine_item_id =
                        Uuid::parse_str(&args.quarantine_item_id).map_err(|error| {
                            anyhow::anyhow!(
                                "invalid quarantine_item_id {}: {}",
                                args.quarantine_item_id,
                                error
                            )
                        })?;
                    let item = postgres::get_quarantine_item(&db, quarantine_item_id).await?;
                    println!("{}", serde_json::to_string(&item)?);
                }
                MemoryCommand::CreateRetrievalTrace(args) => {
                    let workspace = postgres::get_workspace_by_code(&db, &args.workspace).await?;
                    let project = postgres::get_project_by_code(&db, &args.project).await?;
                    let namespace =
                        postgres::get_namespace_by_code(&db, project.project_id, &args.namespace)
                            .await?;
                    let context_pack_id = args
                        .context_pack_id
                        .as_deref()
                        .map(Uuid::parse_str)
                        .transpose()
                        .map_err(|error| anyhow::anyhow!("invalid context_pack_id: {}", error))?;
                    let scope_filter =
                        serde_json::from_str::<serde_json::Value>(&args.scope_filter_json)
                            .context("invalid --scope-filter-json for retrieval trace")?;
                    let candidate_summary =
                        serde_json::from_str::<serde_json::Value>(&args.candidate_summary_json)
                            .context("invalid --candidate-summary-json for retrieval trace")?;
                    let rerank_summary =
                        serde_json::from_str::<serde_json::Value>(&args.rerank_summary_json)
                            .context("invalid --rerank-summary-json for retrieval trace")?;
                    let evidence_sufficiency =
                        serde_json::from_str::<serde_json::Value>(&args.evidence_sufficiency_json)
                            .context("invalid --evidence-sufficiency-json for retrieval trace")?;
                    let source_event_ids_json = serde_json::json!(args.source_event_ids);
                    let artifact_refs_json = serde_json::json!(args.artifact_refs);
                    let message_refs_json = serde_json::json!(args.message_refs);
                    let evidence_span =
                        serde_json::from_str::<serde_json::Value>(&args.evidence_span_json)
                            .context("invalid --evidence-span-json for retrieval trace")?;
                    let trace_payload =
                        serde_json::from_str::<serde_json::Value>(&args.trace_payload_json)
                            .context("invalid --trace-payload-json for retrieval trace")?;
                    let retrieval_trace_id = postgres::create_retrieval_trace(
                        &db,
                        &postgres::RetrievalTraceInsert {
                            workspace_id: workspace.workspace_id,
                            project_id: project.project_id,
                            namespace_id: namespace.namespace_id,
                            context_pack_id,
                            query_text: args.query_text,
                            requested_mode: args.requested_mode,
                            effective_mode: args.effective_mode,
                            scope_filter,
                            candidate_summary,
                            rerank_summary,
                            evidence_sufficiency,
                            source_kind: args.source_kind,
                            source_event_ids: source_event_ids_json,
                            artifact_refs: artifact_refs_json,
                            message_refs: message_refs_json,
                            evidence_span,
                            derivation_kind: args.derivation_kind,
                            schema_version: args.schema_version,
                            final_decision: args.final_decision,
                            temporal_query_epoch_ms: args.temporal_query_epoch_ms,
                            trace_payload,
                        },
                    )
                    .await?;
                    println!("retrieval trace created: {}", retrieval_trace_id);
                }
                MemoryCommand::GetRetrievalTrace(args) => {
                    let retrieval_trace_id =
                        Uuid::parse_str(&args.retrieval_trace_id).map_err(|error| {
                            anyhow::anyhow!(
                                "invalid retrieval_trace_id {}: {}",
                                args.retrieval_trace_id,
                                error
                            )
                        })?;
                    let trace = postgres::get_retrieval_trace(&db, retrieval_trace_id).await?;
                    println!("{}", serde_json::to_string(&trace)?);
                }
                MemoryCommand::Consolidate(args) => {
                    let now_ms = args.now_epoch_ms.unwrap_or_else(|| {
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis() as i64
                    });
                    let report =
                        forgetting::run_consolidation(&db, &args.project, &args.namespace, now_ms)
                            .await?;
                    println!("{}", serde_json::to_string_pretty(&report)?);
                }
                MemoryCommand::RunJob(args) => {
                    let now_ms = args.now_epoch_ms.unwrap_or_else(|| {
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis() as i64
                    });
                    let report = forgetting::run_forgetting_job(
                        &db,
                        &args.project,
                        &args.namespace,
                        args.job_kind,
                        now_ms,
                        args.utility_threshold,
                        args.freshness_threshold,
                        args.stale_days,
                    )
                    .await?;
                    println!("{}", serde_json::to_string_pretty(&report)?);
                }
                MemoryCommand::Prune(args) => {
                    let now_ms = args.now_epoch_ms.unwrap_or_else(|| {
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis() as i64
                    });
                    let mut actions = forgetting::prune_expired_items(
                        &db,
                        &args.project,
                        &args.namespace,
                        now_ms,
                    )
                    .await?;
                    let utility_actions = forgetting::prune_low_utility_ephemeral(
                        &db,
                        &args.project,
                        &args.namespace,
                        args.utility_threshold,
                    )
                    .await?;
                    actions.extend(utility_actions);
                    forgetting::persist_audit_log(&db, &actions, &args.project, &args.namespace)
                        .await?;
                    println!("{}", serde_json::to_string_pretty(&actions)?);
                }
                MemoryCommand::ArchiveCold(args) => {
                    let now_ms = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as i64;
                    let stale_threshold = now_ms - args.stale_days * 86_400_000;
                    let actions = forgetting::archive_to_cold_tier(
                        &db,
                        &args.project,
                        &args.namespace,
                        stale_threshold,
                    )
                    .await?;
                    forgetting::persist_audit_log(&db, &actions, &args.project, &args.namespace)
                        .await?;
                    println!("{}", serde_json::to_string_pretty(&actions)?);
                }
                MemoryCommand::Revalidate(args) => {
                    let actions = forgetting::revalidate_stale_items(
                        &db,
                        &args.project,
                        &args.namespace,
                        args.freshness_threshold,
                    )
                    .await?;
                    forgetting::persist_audit_log(&db, &actions, &args.project, &args.namespace)
                        .await?;
                    println!("{}", serde_json::to_string_pretty(&actions)?);
                }
                MemoryCommand::TouchAccess(args) => {
                    let memory_item_id =
                        Uuid::parse_str(&args.memory_item_id).map_err(|error| {
                            anyhow::anyhow!(
                                "invalid memory_item_id {}: {}",
                                args.memory_item_id,
                                error
                            )
                        })?;
                    forgetting::touch_memory_item_access(&db, memory_item_id).await?;
                    println!("access touched: {memory_item_id}");
                }
                MemoryCommand::ExplainForgetting(args) => {
                    let memory_item_id =
                        Uuid::parse_str(&args.memory_item_id).map_err(|error| {
                            anyhow::anyhow!(
                                "invalid memory_item_id {}: {}",
                                args.memory_item_id,
                                error
                            )
                        })?;
                    let entries = forgetting::explain_forgetting(&db, memory_item_id).await?;
                    if entries.is_empty() {
                        println!("no forgetting actions recorded for {memory_item_id}");
                    } else {
                        println!("{}", serde_json::to_string_pretty(&entries)?);
                    }
                }
                MemoryCommand::TransitionStats(args) => {
                    let report =
                        forgetting::transition_stats(&db, &args.project, &args.namespace).await?;
                    println!("{}", serde_json::to_string_pretty(&report)?);
                }
                MemoryCommand::CohortRisk(args) => {
                    let report =
                        forgetting::cohort_risk(&db, &args.project, &args.namespace).await?;
                    println!("{}", serde_json::to_string_pretty(&report)?);
                }
                MemoryCommand::PolicySimulate(args) => {
                    let report =
                        forgetting::policy_simulate(&db, &args.project, &args.namespace).await?;
                    println!("{}", serde_json::to_string_pretty(&report)?);
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
                ContextCommand::GetPack(args) => {
                    let context_pack_id =
                        Uuid::parse_str(&args.context_pack_id).map_err(|error| {
                            anyhow::anyhow!(
                                "invalid context_pack_id {}: {}",
                                args.context_pack_id,
                                error
                            )
                        })?;
                    let context_pack = postgres::get_context_pack(&db, context_pack_id).await?;
                    println!("{}", serde_json::to_string(&context_pack)?);
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
            VerifyCommand::ProceduralBenchmark(args) => {
                let cfg = config::AppConfig::from_env()?;
                compatibility::assert_supported(&cfg).await?;
                let mut db = postgres::connect_admin(&cfg).await?;
                verify::run_procedural_benchmark(&cfg, &mut db, &args).await?;
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
            ObserveCommand::RegressionExplain(args) => {
                let cfg = config::AppConfig::from_env()?;
                compatibility::assert_supported(&cfg).await?;
                observe::print_regression_explain(&cfg, &args).await?;
            }
            ObserveCommand::CapacityForecast(args) => {
                let cfg = config::AppConfig::from_env()?;
                compatibility::assert_supported(&cfg).await?;
                observe::print_capacity_forecast(&cfg, &args).await?;
            }
            ObserveCommand::GetSnapshot(args) => {
                let cfg = config::AppConfig::from_env()?;
                compatibility::assert_supported(&cfg).await?;
                let db = postgres::connect_admin(&cfg).await?;
                let snapshot_id = Uuid::parse_str(&args.snapshot_id).map_err(|error| {
                    anyhow::anyhow!("invalid snapshot_id {}: {}", args.snapshot_id, error)
                })?;
                let snapshot = postgres::get_observability_snapshot_record(&db, &snapshot_id)
                    .await?
                    .ok_or_else(|| {
                        anyhow::anyhow!("observability snapshot not found: {}", snapshot_id)
                    })?;
                println!("{}", serde_json::to_string(&snapshot)?);
            }
            ObserveCommand::ListSnapshots(args) => {
                let cfg = config::AppConfig::from_env()?;
                compatibility::assert_supported(&cfg).await?;
                let db = postgres::connect_admin(&cfg).await?;
                let records = match (&args.project, &args.namespace) {
                    (Some(project), Some(namespace)) => {
                        postgres::list_observability_snapshots_by_kind_for_scope(
                            &db,
                            &args.kind,
                            "working_state_restore",
                            project,
                            namespace,
                            args.limit,
                        )
                        .await?
                    }
                    _ => {
                        postgres::list_observability_snapshots_by_kinds(
                            &db,
                            &[args.kind.as_str()],
                            args.limit,
                        )
                        .await?
                    }
                };
                if args.ids_only {
                    let snapshot_ids = records
                        .iter()
                        .map(|record| record.snapshot_id)
                        .collect::<Vec<_>>();
                    println!("{}", serde_json::to_string(&snapshot_ids)?);
                } else {
                    println!("{}", serde_json::to_string(&records)?);
                }
            }
            ObserveCommand::SnapshotPreview => {
                let cfg = config::AppConfig::from_env()?;
                compatibility::assert_supported(&cfg).await?;
                observe::print_snapshot_preview(&cfg).await?;
            }
            ObserveCommand::BudgetSnapshotPreview => {
                let cfg = config::AppConfig::from_env()?;
                compatibility::assert_supported(&cfg).await?;
                observe::print_budget_snapshot_preview(&cfg).await?;
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
            ObserveCommand::MaterializeContextPackArtifacts(args) => {
                let cfg = config::AppConfig::from_env()?;
                compatibility::assert_supported(&cfg).await?;
                let mut db = postgres::connect_admin(&cfg).await?;
                retrieval::materialize_pending_context_pack_artifacts(&cfg, &mut db, args.limit)
                    .await?;
            }
            ObserveCommand::ListPendingContextPackArtifacts(args) => {
                let cfg = config::AppConfig::from_env()?;
                compatibility::assert_supported(&cfg).await?;
                let db = postgres::connect_admin(&cfg).await?;
                let context_pack_id = args
                    .context_pack_id
                    .as_deref()
                    .map(Uuid::parse_str)
                    .transpose()
                    .map_err(|error| anyhow::anyhow!("invalid context_pack_id: {}", error))?;
                let records =
                    postgres::list_pending_context_pack_artifacts(&db, args.limit, context_pack_id)
                        .await?;
                println!("{}", serde_json::to_string(&records)?);
            }
            ObserveCommand::ClientBudgetGate(args) => {
                let cfg = config::AppConfig::from_env()?;
                compatibility::assert_supported(&cfg).await?;
                observe::print_client_budget_gate(
                    &cfg,
                    args.enforce_reply_gate,
                    args.thread_id.as_deref(),
                )
                .await?;
            }
            ObserveCommand::ClientBudgetGuard(args) => {
                let cfg = config::AppConfig::from_env()?;
                compatibility::assert_supported(&cfg).await?;
                observe::print_client_budget_guard(
                    &cfg,
                    args.enforce_reply_gate,
                    args.thread_id.as_deref(),
                )
                .await?;
            }
            ObserveCommand::ClientBudgetRootCause(args) => {
                let cfg = config::AppConfig::from_env()?;
                compatibility::assert_supported(&cfg).await?;
                observe::print_client_budget_root_cause(
                    &cfg,
                    args.enforce_reply_gate,
                    args.thread_id.as_deref(),
                )
                .await?;
            }
            ObserveCommand::ClientBudgetHostControlLaunch(args) => {
                let cfg = config::AppConfig::from_env()?;
                compatibility::assert_supported(&cfg).await?;
                observe::print_client_budget_host_control_launch(&cfg, &args).await?;
            }
            ObserveCommand::ClientLimitHourlyBurn(args) => {
                let cfg = config::AppConfig::from_env()?;
                compatibility::assert_supported(&cfg).await?;
                let db = postgres::connect_admin(&cfg).await?;
                token_budget::print_client_limit_hourly_burn(&db, &args).await?;
            }
            ObserveCommand::ClientLimitTrendAnalysis(args) => {
                let cfg = config::AppConfig::from_env()?;
                compatibility::assert_supported(&cfg).await?;
                let db = postgres::connect_admin(&cfg).await?;
                token_budget::print_client_limit_trend_analysis(&db, &args).await?;
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
            ObserveCommand::RelayMemoryWriteOutbox(args) => {
                let cfg = config::AppConfig::from_env()?;
                compatibility::assert_supported(&cfg).await?;
                let db = postgres::connect_admin(&cfg).await?;
                let published = nats::relay_memory_write_outbox(&cfg, &db, args.limit).await?;
                println!("published memory_write_outbox deliveries: {}", published);
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
