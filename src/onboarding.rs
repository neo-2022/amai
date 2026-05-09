use crate::cli::{
    BootstrapAgentPreflightArgs, BootstrapDisconnectArgs, BootstrapOnboardingArgs,
    BootstrapReconnectArgs, McpConfigArgs,
};
use crate::config;
use crate::continuity;
use crate::dashboard::{
    CLIENT_TURN_PRESSURE_ROTATE_STATUS_LABELS, client_turn_pressure_display_status_label,
};
use crate::mcp;
use crate::observe;
use crate::profiles;
use crate::working_state;
use anyhow::{Context, Result, anyhow, bail};
use dirs::home_dir;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{SystemTime, UNIX_EPOCH};
use sysinfo::{Disks, System};
use tokio::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct InstallState {
    package_version: String,
    repo_revision: String,
    client_key: String,
    client_config: String,
    stack_profile: String,
    installed_at_epoch_seconds: u64,
    #[serde(default)]
    memory_bridge_path: Option<String>,
    #[serde(default)]
    memory_bridge_backup_path: Option<String>,
    #[serde(default)]
    startup_instruction_path: Option<String>,
    #[serde(default)]
    startup_instruction_status: Option<String>,
    #[serde(default)]
    startup_contract_path: Option<String>,
    #[serde(default)]
    startup_contract_status: Option<String>,
    #[serde(default)]
    startup_contract_sha256: Option<String>,
    #[serde(default)]
    client_runtime_path: Option<String>,
    #[serde(default)]
    client_runtime_status: Option<String>,
    #[serde(default)]
    agent_preflight_contract_path: Option<String>,
    #[serde(default)]
    agent_preflight_agent_contract_path: Option<String>,
    #[serde(default)]
    agent_preflight_state_path: Option<String>,
    #[serde(default)]
    agent_preflight_contract_status: Option<String>,
    #[serde(default)]
    agent_preflight_contract_sha256: Option<String>,
}

#[derive(Debug, Clone)]
struct InstallMachineSummary {
    cpu_model: String,
    logical_cpus: usize,
    total_memory_gib: f64,
    available_memory_gib: f64,
    memory_type: String,
    available_disk_gib: f64,
}

#[derive(Debug, Clone)]
struct InstallMetricsSummary {
    postgres_query_probe_p95_ms: Option<f64>,
    postgres_connection_usage_ratio: Option<f64>,
    nats_publish_probe_p95_ms: Option<f64>,
    nats_consumer_lag_msgs: Option<f64>,
    qdrant_index_optimize_queue: Option<f64>,
    qdrant_memory_resident_mb: Option<f64>,
    sla_pass: Option<u64>,
    sla_alert: Option<u64>,
    sla_critical: Option<u64>,
    sla_unknown: Option<u64>,
    retrieval_hot_p95_ms: Option<f64>,
    retrieval_cold_p95_ms: Option<f64>,
    load_hot_qps: Option<f64>,
    parser_coverage_ratio: Option<f64>,
    token_savings_percent: Option<f64>,
    token_savings_factor: Option<f64>,
    token_saved_session_total: Option<u64>,
    token_saved_window_total: Option<u64>,
    token_saved_lifetime_total: Option<u64>,
    token_savings_percent_session: Option<f64>,
    token_savings_percent_window: Option<f64>,
    token_savings_percent_lifetime: Option<f64>,
    token_window_label: Option<String>,
    token_headline_title: Option<String>,
    token_headline_value_percent: Option<f64>,
    token_headline_saved_tokens: Option<i64>,
    token_headline_scope_label: Option<String>,
    latest_retrieval_included_reasons: Option<String>,
    latest_retrieval_excluded_reasons: Option<String>,
}

struct DisconnectSummary {
    repo_root: PathBuf,
    install_state_before: Option<InstallState>,
    client_key: String,
    client_display_name: String,
    client_resolution_mode: String,
    client_detection_reason: String,
    client_config: PathBuf,
    backup_file: Option<PathBuf>,
    startup_instructions_removed: Option<StartupInstructionsInstallSummary>,
    client_runtime_removed: Option<ClientRuntimeInstallSummary>,
    config_remove_result: mcp::RemoveConfigResult,
}

#[derive(Default)]
struct FullRemoveSummary {
    systemd_user_unit_removed: bool,
    stack_down_succeeded: bool,
    state_tree_removed: bool,
    install_state_removed: bool,
    repo_root_removed: bool,
    support_root_removed: bool,
    vscode_bridge_removed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StartupArtifactAudit {
    pub status: String,
    pub client_key: String,
    pub startup_instruction_path: Option<PathBuf>,
    pub startup_instruction_exists: bool,
    pub startup_instruction_contains_expected_sha: Option<bool>,
    pub startup_instruction_contains_required_before_tool_call: Option<bool>,
    pub startup_instruction_contains_missing_fail_closed: Option<bool>,
    pub startup_instruction_contains_sha_mismatch_fail_closed: Option<bool>,
    pub startup_instruction_contains_startup_next_action: Option<bool>,
    pub startup_instruction_contains_required_return_task: Option<bool>,
    pub startup_instruction_contains_resume_required_action_kind: Option<bool>,
    pub startup_instruction_contains_execctl_resume_contract_summary: Option<bool>,
    pub startup_instruction_contains_execctl_resume_obligation: Option<bool>,
    pub startup_instruction_contains_execctl_active_lease_summary: Option<bool>,
    pub startup_instruction_contains_lease_owner_state: Option<bool>,
    pub startup_instruction_contains_previous_session_owner_value: Option<bool>,
    pub startup_instruction_contains_previous_session_owner_follow: Option<bool>,
    pub startup_instruction_contains_no_silent_drop: Option<bool>,
    pub startup_instruction_contains_runtime_state_artifact: Option<bool>,
    pub startup_instruction_contains_runtime_state_artifact_version: Option<bool>,
    pub startup_instruction_contains_runtime_state_written_by_tool: Option<bool>,
    pub startup_instruction_contains_runtime_state_source_summary_field: Option<bool>,
    pub startup_instruction_contains_project_task_tree: Option<bool>,
    pub startup_instruction_contains_project_task_tree_summary: Option<bool>,
    pub startup_instruction_contains_project_task_ledger: Option<bool>,
    pub startup_instruction_contains_project_task_ledger_summary: Option<bool>,
    pub startup_instruction_contains_startup_execution_gate: Option<bool>,
    pub startup_instruction_contains_startup_state_fallback_cli: Option<bool>,
    pub startup_instruction_contains_gate_field_enforcement: Option<bool>,
    pub startup_instruction_contains_gate_semantics_consistent: Option<bool>,
    pub startup_contract_path: Option<PathBuf>,
    pub startup_contract_exists: bool,
    pub startup_contract_sha_matches_current_contract: Option<bool>,
    pub install_state_sha_matches_current_contract: Option<bool>,
    pub startup_contract_enforces_fail_closed: Option<bool>,
    pub startup_contract_contains_startup_execution_gate_field: Option<bool>,
    pub startup_contract_contains_startup_next_action_field: Option<bool>,
    pub startup_contract_contains_required_return_task_field: Option<bool>,
    pub startup_contract_contains_resume_required_action_kind: Option<bool>,
    pub startup_contract_contains_active_lease_owner_state_field: Option<bool>,
    pub startup_contract_contains_previous_session_owner_value: Option<bool>,
    pub startup_contract_contains_previous_session_owner_follow: Option<bool>,
    pub startup_contract_contains_no_silent_drop: Option<bool>,
    pub startup_contract_contains_runtime_state_artifact: Option<bool>,
    pub startup_contract_contains_runtime_state_artifact_version: Option<bool>,
    pub startup_contract_contains_startup_execution_gate: Option<bool>,
    pub startup_contract_contains_startup_state_fallback_cli: Option<bool>,
    pub startup_contract_contains_gate_semantics_consistent_field: Option<bool>,
    pub startup_contract_requires_gate_semantics_consistent_true: Option<bool>,
    pub startup_contract_contains_gate_field_enforcement: Option<bool>,
    pub startup_contract_enforces_gate_field_semantics: Option<bool>,
}

#[derive(Debug, Clone)]
struct MemoryBridgeInstallSummary {
    bridge_path: PathBuf,
    backup_path: Option<PathBuf>,
    status: String,
}

pub async fn run(args: &BootstrapOnboardingArgs) -> Result<()> {
    let repo_root = discover_repo_root(args.cwd.as_deref())?;
    let workspace_root = resolve_workspace_root(&repo_root, args.workspace_root.as_deref())?;
    best_effort_cleanup_mcp_orphans(&repo_root).await;
    let remote_mode = args.ssh_destination.is_some();
    if cfg!(windows) && !remote_mode {
        bail!(
            "Local Windows bootstrap install is not supported yet. Use WSL2 for a local stack or pass --ssh-destination for a remote Amai host."
        );
    }
    let client_prompt_allowed = env::var("AMAI_ALLOW_CLIENT_PROMPT").unwrap_or_default() == "1"
        || (!args.yes && interactive_prompt_allowed());
    let client_resolution = resolve_client_target(&repo_root, &args.client, client_prompt_allowed)?;
    let package_version = env!("CARGO_PKG_VERSION").to_string();
    let repo_revision = current_repo_revision(&repo_root).await;
    let mut local_preflight_report: Option<profiles::PreflightReport> = None;
    let mut local_machine_summary: Option<InstallMachineSummary> = None;
    let mut local_metrics_summary: Option<InstallMetricsSummary> = None;
    let mut local_memory_bridge_summary: Option<MemoryBridgeInstallSummary> = None;

    let target = client_resolution.target.clone();
    let output = resolve_output_path(&workspace_root, &target, args.output.as_ref())?;
    let config_args = McpConfigArgs {
        client: client_resolution.client_key.clone(),
        server_name: "amai".to_string(),
        launcher_platform: args.launcher_platform.clone(),
        ssh_destination: args.ssh_destination.clone(),
        remote_repo_root: args.remote_repo_root.clone(),
        command: None,
        cwd: Some(repo_root.clone()),
        output: Some(output.clone()),
    };
    let install_state_before = load_install_state(&repo_root)?;
    let config_existed_before = mcp::client_config_contains_server(&config_args).unwrap_or(false);
    let local_dashboard_url = (!remote_mode).then(|| {
        let bind = env::var("AMI_OBSERVE_BIND").unwrap_or_else(|_| "0.0.0.0:9464".to_string());
        observe::human_dashboard_base_url(&bind)
    });

    if !remote_mode {
        let report = profiles::preflight_report(&repo_root, &args.stack_profile)?;
        if env::var("AMAI_PREFLIGHT_ALREADY_SHOWN").unwrap_or_default() != "1" {
            profiles::print_preflight_report(&report);
        }
        confirm_local_installation(args, &repo_root, &client_resolution, &report)?;
        local_preflight_report = Some(report);
        local_machine_summary = Some(collect_install_machine_summary(&repo_root)?);
        ensure_local_config_files(&repo_root)?;
        dotenvy::from_path_override(repo_root.join(".env"))
            .context("failed to load generated .env for onboarding")?;

        check_dependency(&repo_root, "docker", &["--version"]).await?;
        let cargo_bin = check_dependency(&repo_root, "cargo", &["--version"]).await?;
        let rustc_bin = check_dependency(&repo_root, "rustc", &["--version"]).await?;

        if !args.skip_release_build {
            let mut build_release =
                command_in(&repo_root, cargo_bin.as_os_str(), ["build", "--release"]);
            build_release.env("RUSTC", &rustc_bin);
            run_command("cargo build --release", build_release).await?;
        }

        if !args.skip_stack {
            let mut bootstrap_stack = script_command(
                &repo_root,
                "scripts/bootstrap_stack.sh",
                ["--stack-profile", args.stack_profile.as_str()],
            );
            bootstrap_stack.env("AMAI_SKIP_STACK_PREFLIGHT", "1");
            run_command("bootstrap stack", bootstrap_stack).await?;
        }

        run_command(
            "install managed stack autostart",
            script_command(&repo_root, "scripts/install_stack_autostart.sh", []),
        )
        .await?;

        local_memory_bridge_summary = Some(install_memory_bridge(&repo_root)?);

        if let Ok(cfg) = config::AppConfig::from_env() {
            local_metrics_summary = collect_install_metrics_summary(&cfg).await.ok();
        }
    }
    let backup = maybe_backup_user_global(&output, &target.install_scope)?;
    mcp::write_client_config(&config_args)?;
    let startup_contract_summary = install_startup_contract_artifact(&workspace_root, &repo_root)?;
    let agent_preflight_summary = install_agent_preflight_artifacts(&workspace_root, &repo_root)?;
    let mut startup_instructions_summary =
        install_startup_instructions(&workspace_root, &repo_root, &client_resolution)?;
    let client_runtime_summary = install_client_runtime_artifacts(
        &repo_root,
        &output,
        &client_resolution,
        startup_instructions_summary.as_mut(),
    )?;
    let install_status = build_install_status(
        install_state_before.as_ref(),
        config_existed_before,
        &package_version,
        &repo_revision,
        &client_resolution.client_key,
        &output,
    );
    save_install_state(
        &repo_root,
        &InstallState {
            package_version: package_version.clone(),
            repo_revision: repo_revision.clone(),
            client_key: client_resolution.client_key.clone(),
            client_config: output.display().to_string(),
            stack_profile: args.stack_profile.clone(),
            installed_at_epoch_seconds: current_epoch_seconds(),
            memory_bridge_path: local_memory_bridge_summary
                .as_ref()
                .map(|summary| summary.bridge_path.display().to_string()),
            memory_bridge_backup_path: local_memory_bridge_summary
                .as_ref()
                .and_then(|summary| summary.backup_path.as_ref())
                .map(|path| path.display().to_string()),
            startup_instruction_path: startup_instructions_summary
                .as_ref()
                .map(|summary| summary.output_path.display().to_string()),
            startup_instruction_status: startup_instructions_summary
                .as_ref()
                .map(|summary| summary.status.clone()),
            startup_contract_path: startup_contract_summary
                .as_ref()
                .map(|summary| summary.output_path.display().to_string()),
            startup_contract_status: startup_contract_summary
                .as_ref()
                .map(|summary| summary.status.clone()),
            startup_contract_sha256: startup_contract_summary
                .as_ref()
                .map(|summary| summary.sha256.clone()),
            client_runtime_path: client_runtime_summary
                .as_ref()
                .map(|summary| summary.output_path.display().to_string()),
            client_runtime_status: client_runtime_summary
                .as_ref()
                .map(|summary| summary.status.clone()),
            agent_preflight_contract_path: agent_preflight_summary
                .as_ref()
                .map(|summary| summary.contract_output_path.display().to_string()),
            agent_preflight_agent_contract_path: agent_preflight_summary
                .as_ref()
                .map(|summary| summary.agent_output_path.display().to_string()),
            agent_preflight_state_path: agent_preflight_summary
                .as_ref()
                .map(|summary| summary.state_output_path.display().to_string()),
            agent_preflight_contract_status: agent_preflight_summary
                .as_ref()
                .map(|summary| summary.status.clone()),
            agent_preflight_contract_sha256: agent_preflight_summary
                .as_ref()
                .map(|summary| summary.sha256.clone()),
        },
    )?;

    let release_binary = compiled_binary_path(&repo_root, "target/release", "amai");
    let release_ready = remote_mode || release_binary.is_file();

    println!("Amai готов");
    println!("Версия Amai: {}", package_version);
    println!("Ревизия сборки: {}", repo_revision);
    println!("Результат: {}", install_status);
    if remote_mode {
        println!("Режим подключения: удалённый через SSH");
        println!("Сервер: {}", args.ssh_destination.as_deref().unwrap_or(""));
        println!(
            "Удалённый путь: {}",
            args.remote_repo_root
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_default()
        );
    } else {
        println!("Режим подключения: локальный");
        println!("Файл окружения: {}", repo_root.join(".env").display());
    }
    println!("Клиент: {}", target.display_name);
    if client_resolution.auto_selected {
        println!("Как выбран клиент: автоматически");
        println!(
            "Почему выбран именно он: {}",
            human_client_reason(&client_resolution.reason)
        );
        if !client_resolution.other_detected_clients.is_empty() {
            println!(
                "Другие найденные клиенты: {}",
                client_resolution.other_detected_clients.join(", ")
            );
        }
    } else {
        println!("Как выбран клиент: указан явно");
    }
    println!("Файл подключения: {}", output.display());
    println!(
        "Выбранный профиль: {} ({})",
        target_profile_name(&local_preflight_report, args),
        args.stack_profile
    );
    println!(
        "Где установлен config: {}",
        install_scope_status(&target.install_scope)
    );
    if let Some(summary) = &startup_instructions_summary {
        println!("Startup contract для клиента: {}", summary.status);
        println!(
            "Где лежит startup artifact: {}",
            summary.output_path.display()
        );
        println!(
            "Где лежит startup artifact по scope: {}",
            install_scope_status(&summary.install_scope)
        );
        println!(
            "Auto-start readiness: {}",
            if summary.auto_start_ready {
                "instruction-backed"
            } else {
                "manual_follow_up_required"
            }
        );
        println!("Почему такой режим: {}", summary.reason);
    } else {
        println!("Startup contract для клиента: отдельный artifact не materialized");
    }
    if let Some(summary) = &client_runtime_summary {
        println!("Client runtime artifact: {}", summary.status);
        println!(
            "Где лежит runtime artifact: {}",
            summary.output_path.display()
        );
        println!(
            "Где лежит runtime artifact по scope: {}",
            install_scope_status(&summary.install_scope)
        );
        println!("Почему runtime materialized: {}", summary.reason);
    }
    if let Some(summary) = &startup_contract_summary {
        println!("Machine-readable startup contract: {}", summary.status);
        println!(
            "Где лежит startup contract JSON: {}",
            summary.output_path.display()
        );
        println!(
            "Где лежит startup contract по scope: {}",
            install_scope_status(&summary.install_scope)
        );
        println!("Startup contract SHA-256: {}", summary.sha256);
        println!("Почему contract materialized: {}", summary.reason);
    }
    if let Some(summary) = &agent_preflight_summary {
        println!("Machine-readable agent preflight: {}", summary.status);
        println!(
            "Где лежит agent preflight contract JSON: {}",
            summary.contract_output_path.display()
        );
        println!(
            "Где лежит compact agent preflight JSON: {}",
            summary.agent_output_path.display()
        );
        println!(
            "Где лежит agent preflight runtime state JSON: {}",
            summary.state_output_path.display()
        );
        println!(
            "Где лежит agent preflight по scope: {}",
            install_scope_status(&summary.install_scope)
        );
        println!("Agent preflight SHA-256: {}", summary.sha256);
        println!(
            "Как обновить preflight snapshot: {}",
            summary.refresh_shell_command
        );
        println!("Почему contract materialized: {}", summary.reason);
    }
    if let Some(summary) = &local_memory_bridge_summary {
        println!("Внешний memory bridge: {}", summary.status);
        println!("Файл bridge: {}", summary.bridge_path.display());
        if let Some(path) = &summary.backup_path {
            println!("Резерв старого memory bridge: {}", path.display());
        }
    }
    println!(
        "Release binary готов: {}",
        if release_ready { "да" } else { "нет" }
    );
    if let Some(url) = &local_dashboard_url {
        println!("Живая панель Amai:");
        println!("- запустить: ./scripts/human_dashboard.sh");
        println!("- затем открыть: {url}/");
    }
    if let Some(backup) = backup {
        println!("Резервная копия старого config: {}", backup.display());
    }
    if remote_mode {
        println!("Что делать дальше:");
        println!("- проверьте, что SSH до сервера работает");
        println!("- перезапустите клиент или сделайте Reload Window");
        println!("- попросите клиента обратиться к Amai через MCP");
    } else {
        if let Some(machine) = &local_machine_summary {
            println!();
            println!("Что машина реально показала после установки:");
            println!(
                "- CPU: {} ({} логических потоков)",
                machine.cpu_model, machine.logical_cpus
            );
            println!(
                "- ОЗУ: {:.2} GiB всего, свободно сейчас {:.2} GiB, тип: {}",
                machine.total_memory_gib, machine.available_memory_gib, machine.memory_type
            );
            println!("- Диск: свободно {:.2} GiB", machine.available_disk_gib);
        }
        if let Some(metrics) = &local_metrics_summary {
            println!(
                "- PostgreSQL probe p95: {}",
                format_ms(metrics.postgres_query_probe_p95_ms)
            );
            println!(
                "- PostgreSQL connection usage: {}",
                format_ratio_percent(metrics.postgres_connection_usage_ratio)
            );
            println!(
                "- NATS publish p95: {}",
                format_ms(metrics.nats_publish_probe_p95_ms)
            );
            println!(
                "- NATS consumer lag: {}",
                format_count(metrics.nats_consumer_lag_msgs)
            );
            println!(
                "- Qdrant optimize queue: {}",
                format_count(metrics.qdrant_index_optimize_queue)
            );
            println!(
                "- Qdrant resident memory: {}",
                format_mebibytes(metrics.qdrant_memory_resident_mb)
            );
            println!(
                "- SLA summary: pass={}, alert={}, critical={}, unknown={}",
                format_u64(metrics.sla_pass),
                format_u64(metrics.sla_alert),
                format_u64(metrics.sla_critical),
                format_u64(metrics.sla_unknown)
            );
            if has_post_install_runtime_metrics(metrics) {
                println!(
                    "- Hot retrieval p95: {}",
                    format_ms(metrics.retrieval_hot_p95_ms)
                );
                println!(
                    "- Cold retrieval p95: {}",
                    format_ms(metrics.retrieval_cold_p95_ms)
                );
                println!("- Hot load QPS: {}", format_qps(metrics.load_hot_qps));
                println!(
                    "- Parser coverage: {}",
                    format_ratio_percent(metrics.parser_coverage_ratio)
                );
                println!(
                    "- Token savings: {} ({})",
                    format_percent_value(metrics.token_savings_percent),
                    format_factor(metrics.token_savings_factor)
                );
                if metrics.token_headline_title.is_some()
                    || metrics.token_headline_value_percent.is_some()
                    || metrics.token_headline_saved_tokens.is_some()
                {
                    println!(
                        "- Главный KPI по токенам: {} — {} и {} токенов ({})",
                        metrics
                            .token_headline_title
                            .as_deref()
                            .unwrap_or("ещё нет данных"),
                        format_percent_value(metrics.token_headline_value_percent),
                        format_i64(metrics.token_headline_saved_tokens),
                        metrics
                            .token_headline_scope_label
                            .as_deref()
                            .unwrap_or("рабочее окно")
                    );
                }
                if metrics.token_saved_session_total.is_some()
                    || metrics.token_saved_window_total.is_some()
                    || metrics.token_saved_lifetime_total.is_some()
                {
                    println!(
                        "- Сэкономлено токенов за текущую сессию: {} ({})",
                        format_u64(metrics.token_saved_session_total),
                        format_percent_value(metrics.token_savings_percent_session)
                    );
                    println!(
                        "- Сэкономлено токенов за {}: {} ({})",
                        metrics
                            .token_window_label
                            .as_deref()
                            .unwrap_or("текущее окно"),
                        format_u64(metrics.token_saved_window_total),
                        format_percent_value(metrics.token_savings_percent_window)
                    );
                    println!(
                        "- Сэкономлено токенов за всё время: {} ({})",
                        format_u64(metrics.token_saved_lifetime_total),
                        format_percent_value(metrics.token_savings_percent_lifetime)
                    );
                }
                if let Some(value) = &metrics.latest_retrieval_included_reasons {
                    println!("- Почему последний собранный контекст что-то включил: {value}");
                }
                if let Some(value) = &metrics.latest_retrieval_excluded_reasons {
                    println!("- Почему часть слоёв ничего не добавила: {value}");
                }
            } else {
                println!(
                    "- Метрики поиска пока не показаны: они появятся после первой индексации и первого benchmark-proof."
                );
            }
        } else {
            println!();
            println!("Что машина реально показала после установки:");
            println!(
                "- live-метрики снять не удалось сразу. Обычно это значит, что stack ещё не дал snapshot к этому моменту."
            );
        }
        if let Some(report) = &local_preflight_report {
            println!();
            println!("На что можно рассчитывать в этом режиме:");
            for item in &report.profile.suitable_for {
                println!("- {}", explain_capacity_item(item));
            }
            if report.verdict != "pass" {
                println!("Предупреждение:");
                println!("- этот режим запускается, но без большого запаса по тяжёлым сценариям");
            } else if !report.profile.supports_peak_benchmarks {
                println!("Важно:");
                println!(
                    "- это лёгкий режим: он хорош для удалённого доступа и smoke/demo, но не обещает рекордные benchmark-цифры"
                );
            }
        }
        println!("Что делать дальше:");
        println!("- откройте репозиторий в {}", target.display_name);
        println!(
            "- в {} перезапустите окно или сделайте Reload Window",
            target.display_name
        );
        println!(
            "- попросите {} обратиться к Amai через MCP",
            target.display_name
        );
    }
    Ok(())
}

pub fn print_agent_preflight(args: &BootstrapAgentPreflightArgs) -> Result<()> {
    let repo_root = discover_repo_root(args.cwd.as_deref())?;
    let workspace_root = resolve_workspace_root(&repo_root, args.workspace_root.as_deref())?;
    let Some(summary) = install_agent_preflight_artifacts(&workspace_root, &repo_root)? else {
        bail!(
            "workspace {} does not expose the required Amai onboarding/status docs for agent preflight",
            workspace_root.display()
        );
    };
    let state_text = fs::read_to_string(&summary.state_output_path)
        .with_context(|| format!("failed to read {}", summary.state_output_path.display()))?;
    if args.json {
        println!("{state_text}");
        return Ok(());
    }

    let payload: Value =
        serde_json::from_str(&state_text).context("failed to parse agent preflight state json")?;
    println!("Amai agent preflight готов");
    println!("Workspace: {}", workspace_root.display());
    println!(
        "Machine-readable contract: {}",
        summary.contract_output_path.display()
    );
    println!("Compact contract: {}", summary.agent_output_path.display());
    println!(
        "Runtime state artifact: {}",
        summary.state_output_path.display()
    );
    if let Some(next_stage) =
        payload["agent_preflight_summary"]["next_required_stage"]["label"].as_str()
    {
        println!("Следующий обязательный этап: {next_stage}");
    } else if payload["agent_preflight_summary"]["stage_progress_state"].as_str()
        == Some("all_stages_closed")
    {
        println!("Stage checklist: все этапы закрыты по current status snapshot");
    }
    if let Some(focus) = payload["agent_preflight_summary"]["active_focus"].as_array() {
        if !focus.is_empty() {
            println!("Что сейчас в работе:");
            for item in focus.iter().filter_map(Value::as_str) {
                println!("- {item}");
            }
        }
    }
    if let Some(mechanisms) =
        payload["agent_preflight_summary"]["next_stage_ready_mechanisms"].as_array()
    {
        if !mechanisms.is_empty() {
            println!("Готовые механизмы для следующего этапа:");
            for item in mechanisms.iter().filter_map(Value::as_str) {
                println!("- {item}");
            }
        }
    }
    println!("Как обновить snapshot: {}", summary.refresh_shell_command);
    Ok(())
}

pub async fn reconnect(args: &BootstrapReconnectArgs) -> Result<()> {
    let reconnect_args = BootstrapOnboardingArgs {
        client: args.client.clone(),
        stack_profile: "default".to_string(),
        yes: args.yes,
        launcher_platform: args.launcher_platform.clone(),
        ssh_destination: args.ssh_destination.clone(),
        remote_repo_root: args.remote_repo_root.clone(),
        output: args.output.clone(),
        cwd: args.cwd.clone(),
        workspace_root: args.workspace_root.clone(),
        skip_release_build: true,
        skip_stack: true,
    };
    run(&reconnect_args).await
}

fn resolve_workspace_root(source_repo_root: &Path, explicit: Option<&Path>) -> Result<PathBuf> {
    let Some(path) = explicit else {
        return Ok(source_repo_root.to_path_buf());
    };
    let resolved = if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir()
            .context("failed to resolve current directory for workspace_root")?
            .join(path)
    };
    let canonical = resolved
        .canonicalize()
        .with_context(|| format!("failed to resolve workspace_root {}", resolved.display()))?;
    if !canonical.is_dir() {
        bail!(
            "workspace_root must resolve to an existing directory: {}",
            canonical.display()
        );
    }
    Ok(canonical)
}

fn current_epoch_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs())
        .unwrap_or_default()
}

async fn current_repo_revision(repo_root: &Path) -> String {
    match Command::new("git")
        .arg("rev-parse")
        .arg("--short")
        .arg("HEAD")
        .current_dir(repo_root)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .await
    {
        Ok(output) if output.status.success() => {
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        }
        _ => "unknown".to_string(),
    }
}

fn install_state_path(repo_root: &Path) -> PathBuf {
    env::var_os("AMAI_INSTALL_STATE_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_root.join("state/install_state.json"))
}

fn load_install_state(repo_root: &Path) -> Result<Option<InstallState>> {
    let path = install_state_path(repo_root);
    if !path.is_file() {
        return Ok(None);
    }
    let content =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    let state = serde_json::from_str(&content).context("failed to parse install state json")?;
    Ok(Some(state))
}

pub(crate) fn inspect_startup_artifacts(repo_root: &Path) -> Result<Option<StartupArtifactAudit>> {
    let Some(state) = load_install_state(repo_root)? else {
        return Ok(None);
    };
    let startup_contract_path = state.startup_contract_path.as_ref().map(PathBuf::from);
    let startup_contract_exists = startup_contract_path
        .as_ref()
        .map(|path| path.is_file())
        .unwrap_or(false);
    let expected_contract_sha = if startup_contract_exists {
        let path = startup_contract_path
            .as_ref()
            .expect("startup contract path must exist when marked present");
        let content = fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let payload: Value = serde_json::from_str(&content)
            .with_context(|| format!("failed to parse {}", path.display()))?;
        let workspace_root = payload["repo_root"]
            .as_str()
            .map(PathBuf::from)
            .unwrap_or_else(|| repo_root.to_path_buf());
        startup_contract_for_workspace(&workspace_root, repo_root)?.1
    } else {
        startup_contract_sha256(&mcp::project_chat_startup_contract())?
    };
    let startup_instruction_path = state.startup_instruction_path.as_ref().map(PathBuf::from);
    let startup_instruction_exists = startup_instruction_path
        .as_ref()
        .map(|path| path.is_file())
        .unwrap_or(false);
    let startup_instruction_content = if startup_instruction_exists {
        let path = startup_instruction_path
            .as_ref()
            .expect("startup instruction path must exist when marked present");
        Some(
            fs::read_to_string(path)
                .with_context(|| format!("failed to read {}", path.display()))?,
        )
    } else {
        None
    };
    let (
        startup_instruction_contains_expected_sha,
        startup_instruction_contains_required_before_tool_call,
        startup_instruction_contains_missing_fail_closed,
        startup_instruction_contains_sha_mismatch_fail_closed,
        startup_instruction_contains_startup_next_action,
        startup_instruction_contains_required_return_task,
        startup_instruction_contains_resume_required_action_kind,
        startup_instruction_contains_execctl_resume_contract_summary,
        startup_instruction_contains_execctl_resume_obligation,
        startup_instruction_contains_execctl_active_lease_summary,
        startup_instruction_contains_lease_owner_state,
        startup_instruction_contains_previous_session_owner_value,
        startup_instruction_contains_previous_session_owner_follow,
        startup_instruction_contains_no_silent_drop,
        startup_instruction_contains_runtime_state_artifact,
        startup_instruction_contains_runtime_state_artifact_version,
        startup_instruction_contains_runtime_state_written_by_tool,
        startup_instruction_contains_runtime_state_source_summary_field,
        startup_instruction_contains_project_task_tree,
        startup_instruction_contains_project_task_tree_summary,
        startup_instruction_contains_project_task_ledger,
        startup_instruction_contains_project_task_ledger_summary,
        startup_instruction_contains_startup_execution_gate,
        startup_instruction_contains_startup_state_fallback_cli,
        startup_instruction_contains_gate_field_enforcement,
        startup_instruction_contains_gate_semantics_consistent,
    ) = if startup_instruction_exists {
        let content = startup_instruction_content
            .as_deref()
            .expect("startup instruction content must exist when marked present");
        let instruction_references_required_summary_fields =
            content.contains("required_summary_fields");
        let instruction_references_restored_obligations = content.contains("restored_obligations");
        (
            Some(content.contains(&format!(
                "startup_contract_sha256 = \"{expected_contract_sha}\""
            ))),
            Some(content.contains("workspace_contract_required_before_tool_call = true")),
            Some(content.contains("missing_or_unreadable_fail_closed = true")),
            Some(content.contains("sha256_mismatch_fail_closed = true")),
            Some(content.contains("startup_next_action")),
            Some(
                content.contains("required_return_task")
                    || instruction_references_required_summary_fields
                    || instruction_references_restored_obligations,
            ),
            Some(content.contains("resume_required_return_task")),
            Some(
                content.contains("execctl_resume_contract_summary")
                    || instruction_references_required_summary_fields,
            ),
            Some(
                content.contains("execctl_resume_obligation")
                    || instruction_references_restored_obligations,
            ),
            Some(
                content.contains("execctl_active_lease_summary")
                    || instruction_references_required_summary_fields
                    || instruction_references_restored_obligations,
            ),
            Some(content.contains("lease_owner_state")),
            Some(content.contains("previous_session_owner")),
            Some(content.contains("previous_session_owner_must_follow_startup_next_action = true")),
            Some(content.contains("no_silent_drop = true")),
            Some(content.contains(".amai/continuity/project-chat-startup-state.json")),
            Some(
                content.contains(
                    "workspace_runtime_state_artifact_version = \"workspace-startup-runtime-state-v4\"",
                ) || content.contains(
                    "`workspace_runtime_state_artifact_version` должен быть `workspace-startup-runtime-state-v4`",
                ),
            ),
            Some(content.contains("его пишет `amai_continuity_startup`")),
            Some(
                content.contains("он должен нести `continuity_startup_summary`")
                    || content.contains("он обязан нести `continuity_startup_summary`"),
            ),
            Some(
                content.contains("project_task_tree")
                    || instruction_references_required_summary_fields
                    || instruction_references_restored_obligations,
            ),
            Some(
                content.contains("project_task_tree_summary")
                    || instruction_references_required_summary_fields
                    || instruction_references_restored_obligations,
            ),
            Some(
                content.contains("project_task_ledger")
                    || instruction_references_required_summary_fields
                    || instruction_references_restored_obligations,
            ),
            Some(
                content.contains("project_task_ledger_summary")
                    || instruction_references_required_summary_fields
                    || instruction_references_restored_obligations,
            ),
            Some(content.contains("startup_execution_gate")),
            Some(content.contains("continuity_startup_state.sh --repo-root")),
            Some(
                content.contains("startup_execution_gate.must_follow_startup_next_action")
                    && content.contains("startup_execution_gate.unrelated_work_allowed")
                    && content.contains("startup_execution_gate.must_read_prompt_text_before_reply")
                    && content.contains(
                        "startup_execution_gate.required_action_kind_when_resume_required",
                    )
                    && content.contains("startup_execution_gate.no_silent_drop"),
            ),
            Some(content.contains("gate_semantics_consistent")),
        )
    } else {
        (
            None, None, None, None, None, None, None, None, None, None, None, None, None, None,
            None, None, None, None, None, None, None, None, None, None, None, None,
        )
    };
    let (
        startup_contract_sha_matches_current_contract,
        startup_contract_enforces_fail_closed,
        startup_contract_contains_startup_execution_gate_field,
        startup_contract_contains_startup_next_action_field,
        startup_contract_contains_required_return_task_field,
        startup_contract_contains_resume_required_action_kind,
        startup_contract_contains_active_lease_owner_state_field,
        startup_contract_contains_previous_session_owner_value,
        startup_contract_contains_previous_session_owner_follow,
        startup_contract_contains_no_silent_drop,
        startup_contract_contains_runtime_state_artifact,
        startup_contract_contains_runtime_state_artifact_version,
        startup_contract_contains_startup_execution_gate,
        startup_contract_contains_startup_state_fallback_cli,
        startup_contract_contains_gate_semantics_consistent_field,
        startup_contract_requires_gate_semantics_consistent_true,
        startup_contract_contains_gate_field_enforcement,
        startup_contract_enforces_gate_field_semantics,
    ) = if startup_contract_exists {
        let path = startup_contract_path
            .as_ref()
            .expect("startup contract path must exist when marked present");
        let content = fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let payload: Value = serde_json::from_str(&content)
            .with_context(|| format!("failed to parse {}", path.display()))?;
        let artifact_sha = payload["startup_contract_sha256"].as_str();
        let required_summary_fields = payload["startup_contract"]["required_summary_fields"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let contains_required_summary_field = |name: &str| {
            required_summary_fields
                .iter()
                .any(|field| field.as_str() == Some(name))
        };
        let gate_enforcement = &payload["startup_contract"]["startup_execution_gate_enforcement"];
        let artifact_fail_closed = payload["startup_contract"]["artifact_enforcement"]
                ["missing_or_unreadable_fail_closed"]
                .as_bool()
                .unwrap_or(false)
                && payload["startup_contract"]["artifact_enforcement"]
                    ["sha256_mismatch_fail_closed"]
                    .as_bool()
                    .unwrap_or(false)
                && payload["startup_contract"]["artifact_enforcement"]
                    ["workspace_contract_required_before_tool_call"]
                    .as_bool()
                    .unwrap_or(false);
        (
                Some(artifact_sha == Some(expected_contract_sha.as_str())),
                Some(artifact_fail_closed),
                Some(contains_required_summary_field("startup_execution_gate")),
                Some(contains_required_summary_field("startup_next_action")),
                Some(contains_required_summary_field("required_return_task")),
                Some(
                    payload["startup_contract"]["resume_enforcement"]
                        ["required_action_kind_when_resume_required"]
                        .as_str()
                        == Some("resume_required_return_task"),
                ),
                Some(
                    payload["startup_contract"]["resume_enforcement"]
                        ["active_lease_owner_state_field"]
                        .as_str()
                        == Some("lease_owner_state"),
                ),
                Some(
                    payload["startup_contract"]["resume_enforcement"]
                        ["previous_session_owner_value"]
                        .as_str()
                        == Some("previous_session_owner"),
                ),
                Some(
                    payload["startup_contract"]["resume_enforcement"]
                        ["previous_session_owner_must_follow_startup_next_action"]
                        .as_bool()
                        == Some(true),
                ),
                Some(
                    payload["startup_contract"]["resume_enforcement"]["no_silent_drop"]
                        .as_bool()
                        == Some(true),
                ),
                Some(
                    payload["startup_contract"]["runtime_state_artifact"]
                        ["workspace_runtime_state_relative_path"]
                        .as_str()
                        == Some(".amai/continuity/project-chat-startup-state.json")
                        && payload["startup_contract"]["runtime_state_artifact"]
                            ["workspace_runtime_state_artifact_version"]
                            .as_str()
                            == Some("workspace-startup-runtime-state-v4")
                        && payload["startup_contract"]["runtime_state_artifact"]["written_by_tool"]
                            .as_str()
                            == Some("amai_continuity_startup")
                        && payload["startup_contract"]["runtime_state_artifact"]
                            ["source_summary_field"]
                            .as_str()
                            == Some("continuity_startup_summary"),
                ),
                Some(
                    payload["startup_contract"]["runtime_state_artifact"]
                        ["workspace_runtime_state_artifact_version"]
                        .as_str()
                        == Some("workspace-startup-runtime-state-v4"),
                ),
                Some(
                    payload["startup_contract"]["runtime_state_artifact"]
                        ["startup_execution_gate_field"]
                        .as_str()
                        == Some("startup_execution_gate")
                        && payload["startup_contract"]["runtime_state_artifact"]
                            ["startup_execution_gate_version"]
                            .as_str()
                            == Some("startup-execution-gate-v1"),
                ),
                Some(
                    payload["startup_contract"]["runtime_state_artifact"]
                        ["inspection_fallback_cli"]["command"]
                        .as_str()
                        == Some("continuity startup-state")
                        && payload["startup_contract"]["runtime_state_artifact"]
                            ["inspection_fallback_cli"]["requires_repo_root_argument"]
                            .as_bool()
                            == Some(true)
                        && payload["startup_contract"]["runtime_state_artifact"]
                            ["inspection_fallback_cli"]["json_required"]
                            .as_bool()
                            == Some(true)
                        && payload["startup_contract"]["runtime_state_artifact"]
                            ["inspection_fallback_cli"]["returns_startup_execution_gate"]
                            .as_bool()
                            == Some(true),
                ),
                Some(
                    payload["startup_contract"]["runtime_state_artifact"]
                        ["gate_semantics_consistent_field"]
                        .as_str()
                        == Some("gate_semantics_consistent"),
                ),
                Some(
                    payload["startup_contract"]["runtime_state_artifact"]
                        ["gate_semantics_consistent_true_required"]
                        .as_bool()
                        == Some(true),
                ),
                Some(
                    gate_enforcement["gate_field"].as_str() == Some("startup_execution_gate")
                        && gate_enforcement["action_kind_field"].as_str()
                            == Some("action_kind")
                        && gate_enforcement["blocking_field"].as_str() == Some("blocking")
                        && gate_enforcement["resume_state_field"].as_str()
                            == Some("resume_state")
                        && gate_enforcement["required_return_task_present_field"].as_str()
                            == Some("required_return_task_present")
                        && gate_enforcement["required_return_task_headline_field"].as_str()
                            == Some("required_return_task_headline")
                        && gate_enforcement["required_return_task_next_step_field"].as_str()
                            == Some("required_return_task_next_step")
                        && gate_enforcement["lease_owner_state_field"].as_str()
                            == Some("lease_owner_state")
                        && gate_enforcement["must_follow_field"].as_str()
                            == Some("must_follow_startup_next_action")
                        && gate_enforcement["unrelated_work_allowed_field"].as_str()
                            == Some("unrelated_work_allowed")
                        && gate_enforcement["must_read_prompt_text_before_reply_field"].as_str()
                            == Some("must_read_prompt_text_before_reply")
                        && gate_enforcement["required_action_kind_field"].as_str()
                            == Some("required_action_kind_when_resume_required")
                        && gate_enforcement["no_silent_drop_field"].as_str()
                            == Some("no_silent_drop"),
                ),
                Some(
                    gate_enforcement["blocking_true_requires_must_follow"].as_bool()
                        == Some(true)
                        && gate_enforcement["blocking_true_blocks_unrelated_work"].as_bool()
                            == Some(true)
                        && gate_enforcement["must_follow_true_blocks_unrelated_work"].as_bool()
                        == Some(true)
                        && gate_enforcement["unrelated_work_allowed_false_blocks_unrelated_work"]
                            .as_bool()
                            == Some(true)
                        && gate_enforcement["must_read_prompt_text_true_requires_prompt_before_reply"]
                            .as_bool()
                            == Some(true)
                        && gate_enforcement["required_action_kind_resume_required_value"]
                            .as_str()
                            == Some("resume_required_return_task")
                        && gate_enforcement["no_silent_drop_must_be_true"].as_bool()
                            == Some(true),
                ),
            )
    } else {
        (
            None, None, None, None, None, None, None, None, None, None, None, None, None, None,
            None, None, None, None,
        )
    };

    let install_state_sha_matches_current_contract = state
        .startup_contract_sha256
        .as_deref()
        .map(|sha| sha == expected_contract_sha);
    let hermes_compact_instruction = state.client_key == "hermes"
        && startup_instruction_content
            .as_deref()
            .map_or(false, |content| {
                content.contains("compact contract-pointer")
            });

    let startup_instruction_ok = if hermes_compact_instruction {
        startup_instruction_contains_expected_sha == Some(true)
            && startup_instruction_contains_required_before_tool_call == Some(true)
            && startup_instruction_contains_missing_fail_closed == Some(true)
            && startup_instruction_contains_sha_mismatch_fail_closed == Some(true)
            && startup_instruction_contains_startup_next_action == Some(true)
            && startup_instruction_contains_required_return_task == Some(true)
            && startup_instruction_contains_resume_required_action_kind == Some(true)
            && startup_instruction_contains_execctl_resume_contract_summary == Some(true)
            && startup_instruction_contains_execctl_resume_obligation == Some(true)
            && startup_instruction_contains_runtime_state_artifact == Some(true)
            && startup_instruction_contains_startup_execution_gate == Some(true)
            && startup_instruction_contains_startup_state_fallback_cli == Some(true)
            && startup_instruction_contains_gate_semantics_consistent == Some(true)
    } else {
        startup_instruction_contains_expected_sha == Some(true)
            && startup_instruction_contains_required_before_tool_call == Some(true)
            && startup_instruction_contains_missing_fail_closed == Some(true)
            && startup_instruction_contains_sha_mismatch_fail_closed == Some(true)
            && startup_instruction_contains_startup_next_action == Some(true)
            && startup_instruction_contains_required_return_task == Some(true)
            && startup_instruction_contains_resume_required_action_kind == Some(true)
            && startup_instruction_contains_execctl_resume_contract_summary == Some(true)
            && startup_instruction_contains_execctl_resume_obligation == Some(true)
            && startup_instruction_contains_execctl_active_lease_summary == Some(true)
            && startup_instruction_contains_lease_owner_state == Some(true)
            && startup_instruction_contains_previous_session_owner_value == Some(true)
            && startup_instruction_contains_previous_session_owner_follow == Some(true)
            && startup_instruction_contains_no_silent_drop == Some(true)
            && startup_instruction_contains_runtime_state_artifact == Some(true)
            && startup_instruction_contains_runtime_state_artifact_version == Some(true)
            && startup_instruction_contains_runtime_state_written_by_tool == Some(true)
            && startup_instruction_contains_runtime_state_source_summary_field == Some(true)
            && startup_instruction_contains_project_task_tree == Some(true)
            && startup_instruction_contains_project_task_tree_summary == Some(true)
            && startup_instruction_contains_project_task_ledger == Some(true)
            && startup_instruction_contains_project_task_ledger_summary == Some(true)
            && startup_instruction_contains_startup_execution_gate == Some(true)
            && startup_instruction_contains_startup_state_fallback_cli == Some(true)
            && startup_instruction_contains_gate_field_enforcement == Some(true)
            && startup_instruction_contains_gate_semantics_consistent == Some(true)
    };

    let status = if !startup_instruction_exists {
        "missing_startup_instruction".to_string()
    } else if !startup_contract_exists {
        "missing_startup_contract".to_string()
    } else if !startup_instruction_ok {
        "startup_instruction_drift".to_string()
    } else if startup_contract_sha_matches_current_contract != Some(true)
        || install_state_sha_matches_current_contract != Some(true)
        || startup_contract_enforces_fail_closed != Some(true)
        || startup_contract_contains_startup_execution_gate_field != Some(true)
        || startup_contract_contains_startup_next_action_field != Some(true)
        || startup_contract_contains_required_return_task_field != Some(true)
        || startup_contract_contains_resume_required_action_kind != Some(true)
        || startup_contract_contains_active_lease_owner_state_field != Some(true)
        || startup_contract_contains_previous_session_owner_value != Some(true)
        || startup_contract_contains_previous_session_owner_follow != Some(true)
        || startup_contract_contains_no_silent_drop != Some(true)
        || startup_contract_contains_runtime_state_artifact != Some(true)
        || startup_contract_contains_runtime_state_artifact_version != Some(true)
        || startup_contract_contains_startup_execution_gate != Some(true)
        || startup_contract_contains_startup_state_fallback_cli != Some(true)
        || startup_contract_contains_gate_semantics_consistent_field != Some(true)
        || startup_contract_requires_gate_semantics_consistent_true != Some(true)
        || startup_contract_contains_gate_field_enforcement != Some(true)
        || startup_contract_enforces_gate_field_semantics != Some(true)
    {
        "startup_contract_drift".to_string()
    } else {
        "ok".to_string()
    };

    Ok(Some(StartupArtifactAudit {
        status,
        client_key: state.client_key,
        startup_instruction_path,
        startup_instruction_exists,
        startup_instruction_contains_expected_sha,
        startup_instruction_contains_required_before_tool_call,
        startup_instruction_contains_missing_fail_closed,
        startup_instruction_contains_sha_mismatch_fail_closed,
        startup_instruction_contains_startup_next_action,
        startup_instruction_contains_required_return_task,
        startup_instruction_contains_resume_required_action_kind,
        startup_instruction_contains_execctl_resume_contract_summary,
        startup_instruction_contains_execctl_resume_obligation,
        startup_instruction_contains_execctl_active_lease_summary,
        startup_instruction_contains_lease_owner_state,
        startup_instruction_contains_previous_session_owner_value,
        startup_instruction_contains_previous_session_owner_follow,
        startup_instruction_contains_no_silent_drop,
        startup_instruction_contains_runtime_state_artifact,
        startup_instruction_contains_runtime_state_artifact_version,
        startup_instruction_contains_runtime_state_written_by_tool,
        startup_instruction_contains_runtime_state_source_summary_field,
        startup_instruction_contains_project_task_tree,
        startup_instruction_contains_project_task_tree_summary,
        startup_instruction_contains_project_task_ledger,
        startup_instruction_contains_project_task_ledger_summary,
        startup_instruction_contains_startup_execution_gate,
        startup_instruction_contains_startup_state_fallback_cli,
        startup_instruction_contains_gate_field_enforcement,
        startup_instruction_contains_gate_semantics_consistent,
        startup_contract_path,
        startup_contract_exists,
        startup_contract_sha_matches_current_contract,
        install_state_sha_matches_current_contract,
        startup_contract_enforces_fail_closed,
        startup_contract_contains_startup_execution_gate_field,
        startup_contract_contains_startup_next_action_field,
        startup_contract_contains_required_return_task_field,
        startup_contract_contains_resume_required_action_kind,
        startup_contract_contains_active_lease_owner_state_field,
        startup_contract_contains_previous_session_owner_value,
        startup_contract_contains_previous_session_owner_follow,
        startup_contract_contains_no_silent_drop,
        startup_contract_contains_runtime_state_artifact,
        startup_contract_contains_runtime_state_artifact_version,
        startup_contract_contains_startup_execution_gate,
        startup_contract_contains_startup_state_fallback_cli,
        startup_contract_contains_gate_semantics_consistent_field,
        startup_contract_requires_gate_semantics_consistent_true,
        startup_contract_contains_gate_field_enforcement,
        startup_contract_enforces_gate_field_semantics,
    }))
}

fn save_install_state(repo_root: &Path, state: &InstallState) -> Result<()> {
    let path = install_state_path(repo_root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let content =
        serde_json::to_string_pretty(state).context("failed to serialize install state")?;
    fs::write(&path, content).with_context(|| format!("failed to write {}", path.display()))
}

fn install_memory_bridge(repo_root: &Path) -> Result<MemoryBridgeInstallSummary> {
    #[cfg(not(unix))]
    {
        let _ = repo_root;
        bail!(
            "local memory bridge install is supported only on Unix-like hosts; on Windows use WSL2 or remote client config"
        );
    }
    #[cfg(unix)]
    {
        let bin_dir = home_dir()
            .ok_or_else(|| anyhow!("failed to resolve home directory for memory bridge"))?
            .join(".local/bin");
        fs::create_dir_all(&bin_dir)
            .with_context(|| format!("failed to create {}", bin_dir.display()))?;
        let bridge_path = bin_dir.join("memory");
        let target = compiled_binary_path(repo_root, "target/release", "memory");
        if !target.is_file() {
            bail!(
                "Amai memory bridge requires built release binary: {}; run cargo build --release first",
                target.display()
            );
        }

        let mut backup_path = None;
        let status = if let Ok(current_target) = fs::read_link(&bridge_path) {
            if current_target == target {
                "Amai bridge уже был установлен и остался активным.".to_string()
            } else {
                let backup = bridge_path.with_extension("pre-amai-backup");
                if !backup.exists() {
                    fs::rename(&bridge_path, &backup).with_context(|| {
                        format!(
                            "failed to preserve previous memory bridge {} -> {}",
                            bridge_path.display(),
                            backup.display()
                        )
                    })?;
                } else {
                    fs::remove_file(&bridge_path)
                        .with_context(|| format!("failed to replace {}", bridge_path.display()))?;
                }
                std::os::unix::fs::symlink(&target, &bridge_path).with_context(|| {
                    format!(
                        "failed to create Amai memory bridge {} -> {}",
                        bridge_path.display(),
                        target.display()
                    )
                })?;
                backup_path = Some(backup);
                "Старый memory bridge заменён на Amai.".to_string()
            }
        } else if bridge_path.exists() {
            let backup = bridge_path.with_extension("pre-amai-backup");
            if !backup.exists() {
                fs::rename(&bridge_path, &backup).with_context(|| {
                    format!(
                        "failed to preserve previous memory bridge {} -> {}",
                        bridge_path.display(),
                        backup.display()
                    )
                })?;
            } else {
                fs::remove_file(&bridge_path)
                    .with_context(|| format!("failed to replace {}", bridge_path.display()))?;
            }
            std::os::unix::fs::symlink(&target, &bridge_path).with_context(|| {
                format!(
                    "failed to create Amai memory bridge {} -> {}",
                    bridge_path.display(),
                    target.display()
                )
            })?;
            backup_path = Some(backup);
            "Старый memory executable заменён на Amai.".to_string()
        } else {
            std::os::unix::fs::symlink(&target, &bridge_path).with_context(|| {
                format!(
                    "failed to create Amai memory bridge {} -> {}",
                    bridge_path.display(),
                    target.display()
                )
            })?;
            "Amai memory bridge установлен впервые.".to_string()
        };

        Ok(MemoryBridgeInstallSummary {
            bridge_path,
            backup_path,
            status,
        })
    }
}

fn build_install_status(
    previous_state: Option<&InstallState>,
    config_existed_before: bool,
    package_version: &str,
    repo_revision: &str,
    client_key: &str,
    output: &Path,
) -> String {
    match previous_state {
        Some(previous)
            if previous.package_version == package_version
                && previous.repo_revision == repo_revision
                && previous.client_key == client_key
                && previous.client_config == output.display().to_string() =>
        {
            "Amai уже был установлен. Обновление не требовалось; текущая версия уже актуальна."
                .to_string()
        }
        Some(previous) if previous.client_key == client_key => format!(
            "Amai обновлён: было {} ({}) , стало {} ({}).",
            previous.package_version, previous.repo_revision, package_version, repo_revision
        ),
        _ if config_existed_before => {
            "Amai уже был настроен раньше. Текущая установка аккуратно пересинхронизировала конфигурацию."
                .to_string()
        }
        _ => "Amai установлен впервые.".to_string(),
    }
}

fn human_client_reason(reason: &str) -> String {
    if let Some(value) = reason.strip_prefix("env_var:") {
        return format!("нашёл признак клиента в переменной окружения {value}");
    }
    if let Some(value) = reason.strip_prefix("workspace_marker:") {
        return format!("нашёл рабочий marker {value} в текущем проекте");
    }
    if let Some(value) = reason.strip_prefix("home_marker:") {
        return format!("нашёл marker {value} в домашнем профиле пользователя");
    }
    match reason {
        "explicit_client" => "клиент был указан вручную".to_string(),
        "fallback_client" => {
            "явных признаков не нашлось, поэтому выбран безопасный вариант по умолчанию".to_string()
        }
        other => other.to_string(),
    }
}

fn explain_capacity_item(item: &str) -> String {
    match item {
        "полный локальный bootstrap" => "Полный локальный bootstrap: Amai сможет сам поднять свои внутренние сервисы на этой машине без отдельного внешнего сервера.".to_string(),
        "индексация реальных проектов" => "Индексация реальных проектов: можно подключать настоящие рабочие репозитории, а не только маленькие demo-данные.".to_string(),
        "жёсткие proof и benchmark контуры" => "Жёсткие proof и benchmark-контуры: эта машина подходит не только для запуска, но и для серьёзных проверок и замеров.".to_string(),
        "наблюдаемость и monitoring profile" => "Наблюдаемость и monitoring profile: можно включать метрики и следить за состоянием сервисов, а не работать вслепую.".to_string(),
        "remote MCP" => "Remote MCP: Amai можно держать удалённо, а IDE подключать к нему как к внешнему инструменту.".to_string(),
        "маленьких fixture-проектов" => "Маленькие fixture-проекты: этот режим рассчитан на лёгкие демонстрационные и проверочные корпуса, а не на большие реальные базы кода.".to_string(),
        "smoke и demo" => "Smoke и demo: режим подходит для быстрого показа продукта и лёгких проверок без тяжёлой нагрузки.".to_string(),
        "лёгкого удалённого product path" => "Лёгкий удалённый product path: можно держать Amai на слабом удалённом хосте и пользоваться им как удалённым помощником.".to_string(),
        other => other.to_string(),
    }
}

fn collect_install_machine_summary(repo_root: &Path) -> Result<InstallMachineSummary> {
    let mut system = System::new_all();
    system.refresh_memory();
    let cpu_model = system
        .cpus()
        .first()
        .map(|cpu| cpu.brand().trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "модель CPU не определена".to_string());
    let disks = Disks::new_with_refreshed_list();
    let available_disk_gib = disk_available_for_path(&disks, repo_root)
        .map(bytes_to_gib)
        .unwrap_or_default();
    Ok(InstallMachineSummary {
        cpu_model,
        logical_cpus: system.cpus().len(),
        total_memory_gib: bytes_to_gib(system.total_memory()),
        available_memory_gib: bytes_to_gib(system.available_memory()),
        memory_type: detect_memory_type()
            .unwrap_or_else(|| "система не дала определить автоматически".to_string()),
        available_disk_gib,
    })
}

async fn collect_install_metrics_summary(cfg: &config::AppConfig) -> Result<InstallMetricsSummary> {
    let snapshot = observe::collect_snapshot_preview(cfg).await?;
    Ok(InstallMetricsSummary {
        postgres_query_probe_p95_ms: snapshot["postgres"]["query_probe_p95_ms"].as_f64(),
        postgres_connection_usage_ratio: snapshot["postgres"]["connection_usage_ratio"].as_f64(),
        nats_publish_probe_p95_ms: snapshot["nats"]["publish_probe_p95_ms"].as_f64(),
        nats_consumer_lag_msgs: snapshot["nats"]["consumer_lag_msgs"].as_f64(),
        qdrant_index_optimize_queue: snapshot["qdrant"]["index_optimize_queue"].as_f64(),
        qdrant_memory_resident_mb: snapshot["qdrant"]["memory_resident_bytes"]
            .as_f64()
            .map(|value| value / (1024.0 * 1024.0)),
        sla_pass: snapshot["sla"]["summary"]["pass"].as_u64(),
        sla_alert: snapshot["sla"]["summary"]["alert"].as_u64(),
        sla_critical: snapshot["sla"]["summary"]["critical"].as_u64(),
        sla_unknown: snapshot["sla"]["summary"]["unknown"].as_u64(),
        retrieval_hot_p95_ms: snapshot["latest_retrieval_hot"]["benchmark"]["p95_ms"].as_f64(),
        retrieval_cold_p95_ms: snapshot["latest_retrieval_cold"]["benchmark"]["p95_ms"].as_f64(),
        load_hot_qps: snapshot["latest_retrieval_load_hot"]["load_verification"]["qps"].as_f64(),
        parser_coverage_ratio:
            snapshot["latest_index_project"]["index_project"]["parser_coverage_ratio"].as_f64(),
        token_savings_percent:
            snapshot["latest_token_benchmark"]["token_benchmark"]["savings"]["savings_percent"]
                .as_f64(),
        token_savings_factor:
            snapshot["latest_token_benchmark"]["token_benchmark"]["savings"]["savings_factor"]
                .as_f64(),
        token_saved_session_total: snapshot["token_budget_report"]["token_budget_report"]
            ["current_session"]["total_saved_tokens"]
            .as_u64(),
        token_saved_window_total: snapshot["token_budget_report"]["token_budget_report"]
            ["rolling_window"]["total_saved_tokens"]
            .as_u64(),
        token_saved_lifetime_total: snapshot["token_budget_report"]["token_budget_report"]
            ["lifetime"]["total_saved_tokens"]
            .as_u64(),
        token_savings_percent_session: snapshot["token_budget_report"]["token_budget_report"]
            ["current_session"]["savings_percent"]
            .as_f64(),
        token_savings_percent_window: snapshot["token_budget_report"]["token_budget_report"]
            ["rolling_window"]["savings_percent"]
            .as_f64(),
        token_savings_percent_lifetime: snapshot["token_budget_report"]["token_budget_report"]
            ["lifetime"]["savings_percent"]
            .as_f64(),
        token_window_label: snapshot["token_budget_report"]["token_budget_report"]["profile"]
            ["rolling_window_hours"]
            .as_u64()
            .map(|hours| format!("{hours} ч")),
        token_headline_title: snapshot["token_budget_report"]["token_budget_report"]["headline"]
            ["title"]
            .as_str()
            .map(ToOwned::to_owned),
        token_headline_value_percent: snapshot["token_budget_report"]["token_budget_report"]
            ["headline"]["value_percent"]
            .as_f64(),
        token_headline_saved_tokens: snapshot["token_budget_report"]["token_budget_report"]
            ["headline"]["saved_tokens"]
            .as_i64(),
        token_headline_scope_label: snapshot["token_budget_report"]["token_budget_report"]
            ["headline"]["scope_label"]
            .as_str()
            .map(ToOwned::to_owned),
        latest_retrieval_included_reasons: working_state_reason_summary(
            &snapshot,
            "included_reasons_summary",
            "included",
        ),
        latest_retrieval_excluded_reasons: working_state_reason_summary(
            &snapshot,
            "excluded_reasons_summary",
            "not_included",
        ),
    })
}

fn working_state_reason_summary(
    snapshot: &Value,
    summary_key: &str,
    trace_key: &str,
) -> Option<String> {
    let restore = &snapshot["latest_working_state_restore"]["working_state_restore"];
    if let Some(value) = restore[summary_key]
        .as_str()
        .filter(|value| !value.is_empty())
    {
        return Some(value.to_string());
    }
    let items = restore["latest_decision_trace"][trace_key].as_array()?;
    let parts = items
        .iter()
        .take(3)
        .filter_map(|item| {
            let reason = item["reason"].as_str()?.trim();
            if reason.is_empty() {
                return None;
            }
            let strategy = match item["strategy"].as_str().unwrap_or_default() {
                "exact_documents" => "точные совпадения",
                "symbol_hits" => "совпадения по символам",
                "lexical_chunks" => "текстовые фрагменты",
                "semantic_chunks" => "смысловые фрагменты",
                other => other,
            };
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
        Some(parts.join(" • "))
    }
}

fn detect_memory_type() -> Option<String> {
    if let Some(value) = [
        command_memory_type("dmidecode", &["--type", "17"]),
        command_memory_type("lshw", &["-class", "memory"]),
    ]
    .into_iter()
    .flatten()
    .next()
    {
        return Some(value);
    }
    None
}

fn command_memory_type(program: &str, args: &[&str]) -> Option<String> {
    let output = std::process::Command::new(program)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    extract_memory_generation(&text)
}

fn extract_memory_generation(text: &str) -> Option<String> {
    for candidate in ["DDR5", "LPDDR5", "DDR4", "LPDDR4", "DDR3"] {
        if text.contains(candidate) {
            return Some(candidate.to_string());
        }
    }
    None
}

fn has_post_install_runtime_metrics(metrics: &InstallMetricsSummary) -> bool {
    metrics.retrieval_hot_p95_ms.is_some()
        || metrics.retrieval_cold_p95_ms.is_some()
        || metrics.load_hot_qps.is_some()
        || metrics.token_savings_percent.is_some()
        || metrics.token_saved_lifetime_total.is_some()
}

fn format_ms(value: Option<f64>) -> String {
    value
        .map(|number| format!("{number:.3} ms"))
        .unwrap_or_else(|| "ещё нет данных".to_string())
}

fn format_ratio_percent(value: Option<f64>) -> String {
    value
        .map(|number| format!("{:.2}%", number * 100.0))
        .unwrap_or_else(|| "ещё нет данных".to_string())
}

fn format_percent_value(value: Option<f64>) -> String {
    value
        .map(|number| format!("{number:.2}%"))
        .unwrap_or_else(|| "ещё нет данных".to_string())
}

fn format_mebibytes(value: Option<f64>) -> String {
    value
        .map(|number| format!("{number:.2} MiB"))
        .unwrap_or_else(|| "ещё нет данных".to_string())
}

fn format_count(value: Option<f64>) -> String {
    value
        .map(|number| format!("{number:.0}"))
        .unwrap_or_else(|| "ещё нет данных".to_string())
}

fn format_u64(value: Option<u64>) -> String {
    value
        .map(|number| number.to_string())
        .unwrap_or_else(|| "?".to_string())
}

fn format_i64(value: Option<i64>) -> String {
    value
        .map(|number| number.to_string())
        .unwrap_or_else(|| "?".to_string())
}

fn format_qps(value: Option<f64>) -> String {
    value
        .map(|number| format!("{number:.2} qps"))
        .unwrap_or_else(|| "ещё нет данных".to_string())
}

fn format_factor(value: Option<f64>) -> String {
    value
        .map(|number| format!("{number:.2}x меньше токенов"))
        .unwrap_or_else(|| "ещё нет данных".to_string())
}

fn bytes_to_gib(bytes: u64) -> f64 {
    bytes as f64 / (1024.0 * 1024.0 * 1024.0)
}

fn disk_available_for_path(disks: &Disks, path: &Path) -> Option<u64> {
    let canonical = path.canonicalize().ok()?;
    disks
        .iter()
        .filter(|disk| canonical.starts_with(disk.mount_point()))
        .max_by_key(|disk| disk.mount_point().as_os_str().len())
        .map(|disk| disk.available_space())
}

fn target_profile_name(
    report: &Option<profiles::PreflightReport>,
    args: &BootstrapOnboardingArgs,
) -> String {
    report
        .as_ref()
        .map(|value| value.profile.display_name.clone())
        .unwrap_or_else(|| args.stack_profile.clone())
}

fn confirm_local_installation(
    args: &BootstrapOnboardingArgs,
    repo_root: &Path,
    client_resolution: &ClientResolution,
    report: &profiles::PreflightReport,
) -> Result<()> {
    if args.yes {
        return Ok(());
    }

    println!();
    println!("Если продолжить, Amai сделает следующее:");
    println!("- создаст или досинхронизирует файл .env");
    println!("- поднимет локальный stack, если он ещё не поднят");
    println!("- при необходимости соберёт release binary");
    println!(
        "- подготовит MCP config для клиента: {}",
        client_resolution.target.display_name
    );
    println!("- рабочий корень установки: {}", repo_root.display());
    println!("- выбранный профиль: {}", report.profile.display_name);
    println!();
    print!("Если согласны продолжать, напишите ДА и нажмите Enter: ");
    io::stdout().flush().context("failed to flush stdout")?;

    let mut answer = String::new();
    if !io::stdin().is_terminal() {
        println!("Подсказка: для автоматизации можно передать --yes.");
    }
    let bytes_read = io::stdin()
        .read_line(&mut answer)
        .context("failed to read confirmation input")?;
    if bytes_read == 0 {
        bail!("installation cancelled because no confirmation was provided");
    }
    let normalized = answer.trim();
    let approved = matches!(normalized, "ДА" | "да" | "Да" | "yes" | "YES" | "y" | "Y");
    if !approved {
        bail!("installation cancelled by user before any changes were made");
    }

    Ok(())
}

pub async fn disconnect(args: &BootstrapDisconnectArgs) -> Result<()> {
    let summary = disconnect_summary(args).await?;
    print_disconnect_summary(&summary)?;
    Ok(())
}

pub async fn remove(args: &BootstrapDisconnectArgs) -> Result<()> {
    let summary = disconnect_summary(args).await?;
    print_disconnect_summary(&summary)?;
    if !full_remove_mode_enabled() {
        return Ok(());
    }
    let full_remove = full_remove_runtime(&summary).await?;
    println!("full_remove: true");
    println!(
        "systemd_user_unit_removed: {}",
        full_remove.systemd_user_unit_removed
    );
    println!("stack_down_succeeded: {}", full_remove.stack_down_succeeded);
    println!("state_tree_removed: {}", full_remove.state_tree_removed);
    println!(
        "install_state_removed: {}",
        full_remove.install_state_removed
    );
    println!("repo_root_removed: {}", full_remove.repo_root_removed);
    println!("support_root_removed: {}", full_remove.support_root_removed);
    println!(
        "vscode_bridge_removed: {}",
        full_remove.vscode_bridge_removed
    );
    println!("next_step_1: verify that Amai is no longer installed on this host");
    println!("next_step_2: reinstall from GitHub only if you intentionally want Amai back");
    Ok(())
}

fn full_remove_mode_enabled() -> bool {
    matches!(
        env::var("AMAI_BOOTSTRAP_REMOVE_MODE").ok().as_deref(),
        Some("1" | "true" | "TRUE" | "full" | "FULL")
    )
}

async fn disconnect_summary(args: &BootstrapDisconnectArgs) -> Result<DisconnectSummary> {
    let repo_root = discover_repo_root(args.cwd.as_deref())?;
    best_effort_cleanup_mcp_orphans(&repo_root).await;
    let install_state_before = load_install_state(&repo_root)?;
    let client_resolution = resolve_client_target(&repo_root, &args.client, false)?;
    let target = client_resolution.target.clone();
    let output = resolve_output_path(&repo_root, &target, args.output.as_ref())?;
    let backup = maybe_backup_user_global(&output, &target.install_scope)?;
    let client_runtime_removed =
        remove_client_runtime_artifacts(&repo_root, &client_resolution.client_key, &output)?;
    let startup_instructions_removed =
        remove_startup_instructions(&repo_root, &target).unwrap_or(None);

    let result = mcp::remove_client_config(
        &McpConfigArgs {
            client: client_resolution.client_key.clone(),
            server_name: "amai".to_string(),
            launcher_platform: "auto".to_string(),
            ssh_destination: None,
            remote_repo_root: None,
            command: None,
            cwd: Some(repo_root.clone()),
            output: Some(output.clone()),
        },
        args.purge_empty_file,
    )?;

    Ok(DisconnectSummary {
        repo_root,
        install_state_before,
        client_key: client_resolution.client_key,
        client_display_name: target.display_name,
        client_resolution_mode: if client_resolution.auto_selected {
            "auto_detected".to_string()
        } else {
            "explicit".to_string()
        },
        client_detection_reason: client_resolution.reason,
        client_config: output,
        backup_file: backup,
        startup_instructions_removed,
        client_runtime_removed,
        config_remove_result: result,
    })
}

fn print_disconnect_summary(summary: &DisconnectSummary) -> Result<()> {
    println!("disconnect completed");
    println!("client: {}", summary.client_key);
    println!("client_display_name: {}", summary.client_display_name);
    println!("client_resolution_mode: {}", summary.client_resolution_mode);
    println!(
        "client_detection_reason: {}",
        summary.client_detection_reason
    );
    println!("client_config: {}", summary.client_config.display());
    println!("server_removed: {}", summary.config_remove_result.removed);
    println!("file_purged: {}", summary.config_remove_result.purged_file);
    if let Some(startup_summary) = &summary.startup_instructions_removed {
        println!("startup_instruction_removed: true");
        println!(
            "startup_instruction_path: {}",
            startup_summary.output_path.display()
        );
        println!("startup_instruction_status: {}", startup_summary.status);
    } else {
        println!("startup_instruction_removed: false");
    }
    if let Some(client_runtime_summary) = &summary.client_runtime_removed {
        println!("client_runtime_removed: true");
        println!(
            "client_runtime_path: {}",
            client_runtime_summary.output_path.display()
        );
        println!("client_runtime_status: {}", client_runtime_summary.status);
    } else {
        println!("client_runtime_removed: false");
    }
    if let Some(state) = &summary.install_state_before {
        if let Some(startup_contract_path) = &state.startup_contract_path {
            println!("startup_contract_path: {}", startup_contract_path);
        }
        if let Some(startup_contract_status) = &state.startup_contract_status {
            println!("startup_contract_status: {}", startup_contract_status);
        }
        if let Some(startup_contract_sha256) = &state.startup_contract_sha256 {
            println!("startup_contract_sha256: {}", startup_contract_sha256);
        }
        if let Some(memory_bridge_path) = &state.memory_bridge_path {
            println!("memory_bridge: {}", memory_bridge_path);
        }
        if let Some(memory_bridge_backup_path) = &state.memory_bridge_backup_path {
            println!("memory_bridge_backup: {}", memory_bridge_backup_path);
        }
    }
    if let Some(backup) = &summary.backup_file {
        println!("backup_file: {}", backup.display());
    }
    if let Some(memory_bridge_summary) = restore_memory_bridge(
        summary.repo_root.as_path(),
        summary.install_state_before.as_ref(),
    )? {
        println!("memory_bridge_restore: {}", memory_bridge_summary);
    }
    println!("next_step_1: reload the client window or restart the client");
    println!("next_step_2: verify that Amai is no longer listed as an MCP server");
    Ok(())
}

async fn full_remove_runtime(summary: &DisconnectSummary) -> Result<FullRemoveSummary> {
    let repo_root = &summary.repo_root;
    let managed_clone_root = managed_clone_root()?;
    let mut full_remove = FullRemoveSummary::default();
    let install_state_path = install_state_path(repo_root);
    let install_state_present_before = install_state_path.exists();

    if summary.client_key == "vscode" {
        full_remove.vscode_bridge_removed = remove_vscode_bridge_install().await?;
    }

    full_remove.systemd_user_unit_removed = remove_stack_autostart_unit().await?;
    full_remove.stack_down_succeeded = compose_down_stack(repo_root).await?;

    let state_dir = repo_root.join("state");
    let tmp_dir = repo_root.join("tmp");
    let mut state_tree_removed = false;
    state_tree_removed |= remove_tree_forcefully(&state_dir).await?;
    state_tree_removed |= remove_tree_forcefully(&tmp_dir).await?;
    full_remove.state_tree_removed = state_tree_removed;

    if install_state_path.exists() {
        fs::remove_file(&install_state_path)
            .with_context(|| format!("failed to remove {}", install_state_path.display()))?;
        full_remove.install_state_removed = true;
    } else if install_state_present_before {
        full_remove.install_state_removed = true;
    }

    if repo_root == &managed_clone_root {
        let fallback_cwd = managed_clone_root
            .parent()
            .map(Path::to_path_buf)
            .or_else(home_dir)
            .unwrap_or_else(std::env::temp_dir);
        env::set_current_dir(&fallback_cwd).with_context(|| {
            format!(
                "failed to leave managed clone root before removal: {}",
                fallback_cwd.display()
            )
        })?;
        if remove_tree_forcefully(repo_root).await? {
            full_remove.repo_root_removed = true;
        }
        if let Some(parent) = managed_clone_root.parent()
            && parent.exists()
            && is_directory_empty(parent)?
        {
            fs::remove_dir(parent)
                .with_context(|| format!("failed to remove empty {}", parent.display()))?;
            full_remove.support_root_removed = true;
        }
    }

    Ok(full_remove)
}

async fn remove_stack_autostart_unit() -> Result<bool> {
    let home = home_dir().ok_or_else(|| anyhow!("failed to resolve home directory"))?;
    let unit_path = home.join(".config/systemd/user/amai-stack.service");
    if command_exists("systemctl").await {
        let _ = Command::new("systemctl")
            .arg("--user")
            .arg("disable")
            .arg("--now")
            .arg("amai-stack.service")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await;
        let _ = Command::new("systemctl")
            .arg("--user")
            .arg("daemon-reload")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await;
    }
    if unit_path.exists() {
        fs::remove_file(&unit_path)
            .with_context(|| format!("failed to remove {}", unit_path.display()))?;
        return Ok(true);
    }
    Ok(false)
}

async fn compose_down_stack(repo_root: &Path) -> Result<bool> {
    if !repo_root.join("compose.yaml").is_file() {
        return Ok(false);
    }
    let status = Command::new("docker")
        .arg("compose")
        .arg("--profile")
        .arg("monitoring")
        .arg("down")
        .arg("--remove-orphans")
        .arg("--volumes")
        .current_dir(repo_root)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await;
    match status {
        Ok(result) if result.success() => Ok(true),
        Ok(_) | Err(_) => Ok(false),
    }
}

async fn remove_vscode_bridge_install() -> Result<bool> {
    let home = home_dir().ok_or_else(|| anyhow!("failed to resolve home directory"))?;
    let mut removed_any = false;
    let mut seen_roots = std::collections::BTreeSet::new();
    let extension_roots = [
        home.join(".vscode/extensions"),
        home.join(".vscode-oss/extensions"),
    ];

    for extensions_root in extension_roots {
        if !seen_roots.insert(extensions_root.clone()) {
            continue;
        }
        let registry_path = extensions_root.join("extensions.json");

        if extensions_root.is_dir() {
            for entry in fs::read_dir(&extensions_root)
                .with_context(|| format!("failed to read {}", extensions_root.display()))?
            {
                let entry = entry?;
                let path = entry.path();
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if !(name.starts_with("amai.amai-vscode-bridge-")
                    || name.starts_with("art-local.amai-vscode-bridge-"))
                {
                    continue;
                }
                if path.is_dir() {
                    fs::remove_dir_all(&path)
                        .with_context(|| format!("failed to remove {}", path.display()))?;
                } else {
                    fs::remove_file(&path)
                        .with_context(|| format!("failed to remove {}", path.display()))?;
                }
                removed_any = true;
            }
        }

        if registry_path.is_file() {
            let raw = fs::read_to_string(&registry_path)
                .with_context(|| format!("failed to read {}", registry_path.display()))?;
            let mut entries: Vec<Value> =
                serde_json::from_str(&raw).context("failed to parse VS Code extensions registry")?;
            let before_len = entries.len();
            entries.retain(|entry| {
                let Some(identifier) = entry.get("identifier") else {
                    return true;
                };
                let Some(id) = identifier.get("id").and_then(Value::as_str) else {
                    return true;
                };
                id != "amai.amai-vscode-bridge" && id != "art-local.amai-vscode-bridge"
            });
            if entries.len() != before_len {
                fs::write(
                    &registry_path,
                    serde_json::to_string_pretty(&entries)? + "\n",
                )
                .with_context(|| format!("failed to write {}", registry_path.display()))?;
                removed_any = true;
            }
        }
    }

    Ok(removed_any)
}

fn managed_clone_root() -> Result<PathBuf> {
    if let Some(explicit) = env::var_os("AMAI_GITHUB_CLONE_DIR") {
        return Ok(PathBuf::from(explicit));
    }
    let home = home_dir().ok_or_else(|| anyhow!("failed to resolve home directory"))?;
    Ok(home.join(".local/share/amai/repo"))
}

fn is_directory_empty(path: &Path) -> Result<bool> {
    Ok(fs::read_dir(path)
        .with_context(|| format!("failed to read {}", path.display()))?
        .next()
        .is_none())
}

async fn remove_tree_forcefully(path: &Path) -> Result<bool> {
    if !path.exists() {
        return Ok(false);
    }
    match fs::remove_dir_all(path) {
        Ok(_) => return Ok(true),
        Err(error) if error.kind() != io::ErrorKind::PermissionDenied => {
            return Err(error).with_context(|| format!("failed to remove {}", path.display()));
        }
        Err(_) => {}
    }

    if command_exists("podman").await {
        let status = Command::new("podman")
            .arg("unshare")
            .arg("rm")
            .arg("-rf")
            .arg(path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
            .with_context(|| {
                format!("failed to execute podman unshare rm for {}", path.display())
            })?;
        if status.success() {
            return Ok(true);
        }
    }

    fs::remove_dir_all(path).with_context(|| format!("failed to remove {}", path.display()))?;
    Ok(true)
}

async fn command_exists(command: &str) -> bool {
    Command::new("sh")
        .arg("-c")
        .arg(format!(
            "command -v {} >/dev/null 2>&1",
            shell_escape(command)
        ))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .map(|status| status.success())
        .unwrap_or(false)
}

fn shell_escape(input: &str) -> String {
    if input
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || b"._-/".contains(&byte))
    {
        input.to_string()
    } else {
        format!("'{}'", input.replace('\'', "'\"'\"'"))
    }
}

fn restore_memory_bridge(repo_root: &Path, state: Option<&InstallState>) -> Result<Option<String>> {
    #[cfg(not(unix))]
    {
        let _ = repo_root;
        let _ = state;
        return Ok(None);
    }
    #[cfg(unix)]
    {
        let Some(state) = state else {
            return Ok(None);
        };
        let Some(bridge_path) = state.memory_bridge_path.as_ref().map(PathBuf::from) else {
            return Ok(None);
        };
        let expected_target = compiled_binary_path(repo_root, "target/release", "memory");
        let current_target = match fs::read_link(&bridge_path) {
            Ok(target) => target,
            Err(_) => return Ok(None),
        };
        if current_target != expected_target {
            return Ok(None);
        }
        if let Some(backup) = state.memory_bridge_backup_path.as_ref().map(PathBuf::from)
            && backup.exists()
        {
            fs::remove_file(&bridge_path)
                .with_context(|| format!("failed to remove {}", bridge_path.display()))?;
            fs::rename(&backup, &bridge_path).with_context(|| {
                format!(
                    "failed to restore previous memory bridge {} -> {}",
                    backup.display(),
                    bridge_path.display()
                )
            })?;
            return Ok(Some("предыдущий memory bridge восстановлен".to_string()));
        }
        fs::remove_file(&bridge_path)
            .with_context(|| format!("failed to remove {}", bridge_path.display()))?;
        Ok(Some(
            "Amai memory bridge удалён без восстановления старого bridge".to_string(),
        ))
    }
}

fn compiled_binary_path(repo_root: &Path, directory: &str, stem: &str) -> PathBuf {
    let suffix = std::env::consts::EXE_SUFFIX;
    repo_root.join(directory).join(format!("{stem}{suffix}"))
}

fn discover_repo_root(explicit: Option<&Path>) -> Result<PathBuf> {
    config::discover_repo_root(explicit)
}

fn ensure_local_config_files(repo_root: &Path) -> Result<()> {
    let example = repo_root.join(".env.example");
    let env_path = repo_root.join(".env");
    let example_content = fs::read_to_string(&example)
        .with_context(|| format!("failed to read {}", example.display()))?;
    if !env_path.is_file() {
        fs::write(&env_path, &example_content)
            .with_context(|| format!("failed to create {}", env_path.display()))?;
        return Ok(());
    }

    let current_content = fs::read_to_string(&env_path)
        .with_context(|| format!("failed to read {}", env_path.display()))?;
    let existing_keys = env_keys(&current_content);
    let mut missing_lines = Vec::new();
    for line in example_content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || !trimmed.contains('=') {
            continue;
        }
        let key = trimmed
            .split_once('=')
            .map(|(key, _)| key.trim())
            .unwrap_or("");
        if !existing_keys.contains(key) {
            missing_lines.push(trimmed.to_string());
        }
    }

    if missing_lines.is_empty() {
        return Ok(());
    }

    let mut merged = current_content;
    if !merged.ends_with('\n') {
        merged.push('\n');
    }
    for line in missing_lines {
        merged.push_str(&line);
        merged.push('\n');
    }
    fs::write(&env_path, merged).with_context(|| format!("failed to update {}", env_path.display()))
}

fn env_keys(content: &str) -> BTreeSet<String> {
    content
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') || !trimmed.contains('=') {
                return None;
            }
            let (key, _) = trimmed.split_once('=')?;
            Some(key.trim().to_string())
        })
        .collect()
}

async fn check_dependency(repo_root: &Path, program: &str, args: &[&str]) -> Result<PathBuf> {
    let resolved_program = resolve_program_path(repo_root, program).await?;
    let status = Command::new(&resolved_program)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .with_context(|| format!("failed to start dependency check for {program}"))?;
    if !status.success() {
        bail!("{program} is required for onboarding but is not available");
    }
    Ok(resolved_program)
}

async fn resolve_program_path(repo_root: &Path, program: &str) -> Result<PathBuf> {
    let resolver_script = match program {
        "cargo" => Some(repo_root.join("scripts/resolve_cargo.sh")),
        "rustc" => Some(repo_root.join("scripts/resolve_rustc.sh")),
        _ => None,
    };
    let Some(resolver_script) = resolver_script else {
        return Ok(PathBuf::from(program));
    };

    let output = Command::new(&resolver_script)
        .current_dir(repo_root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .with_context(|| format!("failed to run resolver for {program}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if stderr.is_empty() {
            bail!("{program} is required for onboarding but is not available");
        }
        bail!("{stderr}");
    }
    let resolved = String::from_utf8(output.stdout)
        .with_context(|| format!("resolver for {program} returned non-utf8 output"))?;
    let trimmed = resolved.trim();
    if trimmed.is_empty() {
        bail!("{program} is required for onboarding but is not available");
    }
    Ok(PathBuf::from(trimmed))
}

async fn best_effort_cleanup_mcp_orphans(repo_root: &Path) {
    let script_path = repo_root.join("scripts/cleanup_mcp_orphans.sh");
    if !script_path.is_file() {
        return;
    }

    let status = Command::new(&script_path)
        .arg(repo_root)
        .current_dir(repo_root)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await;

    match status {
        Ok(result) if result.success() => {}
        Ok(result) => {
            tracing::warn!(
                repo_root = %repo_root.display(),
                status = %result,
                "best-effort MCP orphan cleanup returned non-success"
            );
        }
        Err(error) => {
            tracing::warn!(
                repo_root = %repo_root.display(),
                error = %error,
                "best-effort MCP orphan cleanup failed to start"
            );
        }
    }
}

fn command_in<const N: usize, S: AsRef<OsStr>>(
    repo_root: &Path,
    program: S,
    args: [&str; N],
) -> Command {
    let mut command = Command::new(program);
    command.current_dir(repo_root);
    command.args(args);
    command.stdout(Stdio::inherit());
    command.stderr(Stdio::inherit());
    command
}

fn script_command<const N: usize>(
    repo_root: &Path,
    relative_path: &str,
    args: [&str; N],
) -> Command {
    let mut command = Command::new(repo_root.join(relative_path));
    command.current_dir(repo_root);
    command.args(args);
    command.stdout(Stdio::inherit());
    command.stderr(Stdio::inherit());
    command
}

async fn run_command(label: &str, mut command: Command) -> Result<()> {
    let status = command
        .status()
        .await
        .with_context(|| format!("failed to execute {label}"))?;
    if !status.success() {
        return Err(anyhow!("{label} failed with status {status}"));
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
struct ClientTargetsManifest {
    auto_detection: AutoDetectionConfig,
    clients: BTreeMap<String, ClientTarget>,
}

#[derive(Debug, Clone, Deserialize)]
struct AutoDetectionConfig {
    fallback_client: String,
}

#[derive(Debug, Clone, Deserialize)]
struct ClientTarget {
    display_name: String,
    default_output: String,
    install_scope: String,
    priority: i64,
    detect_env_vars: Vec<String>,
    detect_workspace_markers: Vec<String>,
    detect_home_markers: Vec<String>,
    #[serde(default)]
    startup_instructions: Option<ClientStartupInstructions>,
}

#[derive(Debug, Clone, Deserialize)]
struct ClientStartupInstructions {
    mode: String,
    default_output: String,
    install_scope: String,
    format: String,
}

#[derive(Debug, Clone)]
struct StartupInstructionsInstallSummary {
    status: String,
    output_path: PathBuf,
    install_scope: String,
    auto_start_ready: bool,
    reason: String,
}

#[derive(Debug, Clone)]
struct ClientRuntimeInstallSummary {
    status: String,
    output_path: PathBuf,
    install_scope: String,
    reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct HermesRuntimeArtifact {
    profile_name: String,
    profile_dir: String,
    previous_active_profile: String,
}

#[derive(Debug, Clone)]
struct HermesProfileInstallSummary {
    profile_name: String,
    profile_dir: PathBuf,
    #[allow(dead_code)]
    runtime_artifact_path: PathBuf,
}

#[derive(Debug, Clone)]
struct StartupContractInstallSummary {
    status: String,
    output_path: PathBuf,
    install_scope: String,
    reason: String,
    sha256: String,
}

#[derive(Debug, Clone)]
struct AgentPreflightInstallSummary {
    status: String,
    contract_output_path: PathBuf,
    agent_output_path: PathBuf,
    state_output_path: PathBuf,
    install_scope: String,
    reason: String,
    sha256: String,
    refresh_shell_command: String,
}

#[derive(Debug, Clone)]
struct ClientResolution {
    client_key: String,
    target: ClientTarget,
    auto_selected: bool,
    reason: String,
    other_detected_clients: Vec<String>,
}

fn load_client_targets_manifest(repo_root: &Path) -> Result<ClientTargetsManifest> {
    let manifest_path = repo_root.join("config/client_targets.toml");
    let content = fs::read_to_string(&manifest_path)
        .with_context(|| format!("failed to read {}", manifest_path.display()))?;
    toml::from_str(&content).context("failed to parse config/client_targets.toml")
}

fn resolve_client_target(
    repo_root: &Path,
    requested_client: &str,
    allow_interactive_choice: bool,
) -> Result<ClientResolution> {
    let manifest = load_client_targets_manifest(repo_root)?;
    let requested_key = requested_client.trim().to_ascii_lowercase();
    if requested_key != "auto" {
        let target = manifest
            .clients
            .get(&requested_key)
            .cloned()
            .ok_or_else(|| {
                anyhow!(
                    "unsupported onboarding client target: {requested_key}; register it in config/client_targets.toml"
                )
            })?;
        return Ok(ClientResolution {
            client_key: requested_key,
            target,
            auto_selected: false,
            reason: "explicit_client".to_string(),
            other_detected_clients: Vec::new(),
        });
    }

    let home = dirs::home_dir().ok_or_else(|| anyhow!("failed to resolve user home directory"))?;
    let mut detected = Vec::new();
    for (client_key, target) in &manifest.clients {
        if let Some(score) = detection_score(repo_root, &home, target) {
            let reason = detection_reason(repo_root, &home, target)
                .unwrap_or_else(|| "auto_detected".to_string());
            detected.push((client_key.clone(), target.clone(), score, reason));
        }
    }

    detected.sort_by(|left, right| right.2.cmp(&left.2).then_with(|| left.0.cmp(&right.0)));

    if !detected.is_empty() {
        let selected = if allow_interactive_choice && detected.len() > 1 {
            choose_detected_client(&detected)?
        } else {
            0
        };
        let other_detected_clients = detected
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != selected)
            .map(|(_, (_, target, _, _))| target.display_name.clone())
            .collect::<Vec<_>>();
        let (client_key, target, _, reason) = detected.remove(selected);
        return Ok(ClientResolution {
            client_key,
            target,
            auto_selected: true,
            reason,
            other_detected_clients,
        });
    }

    let fallback_key = manifest
        .auto_detection
        .fallback_client
        .trim()
        .to_ascii_lowercase();
    let target = manifest.clients.get(&fallback_key).cloned().ok_or_else(|| {
        anyhow!(
            "fallback onboarding client target is missing from config/client_targets.toml: {fallback_key}"
        )
    })?;
    Ok(ClientResolution {
        client_key: fallback_key,
        target,
        auto_selected: true,
        reason: "fallback_client".to_string(),
        other_detected_clients: Vec::new(),
    })
}

fn choose_detected_client(detected: &[(String, ClientTarget, i64, String)]) -> Result<usize> {
    println!();
    println!("Обнаружено несколько подходящих клиентов:");
    for (index, (_, target, _, reason)) in detected.iter().enumerate() {
        println!(
            "{}. {} ({})",
            index + 1,
            target.display_name,
            human_client_reason(reason)
        );
    }
    println!();
    print!(
        "Введите номер клиента, который нужно настроить сейчас. Нажмите Enter, чтобы взять рекомендуемый вариант 1: "
    );
    io::stdout().flush().context("failed to flush stdout")?;

    let mut answer = String::new();
    let bytes_read = io::stdin()
        .read_line(&mut answer)
        .context("failed to read client selection input")?;
    if bytes_read == 0 {
        return Ok(0);
    }
    let trimmed = answer.trim();
    if trimmed.is_empty() {
        return Ok(0);
    }
    let index = trimmed
        .parse::<usize>()
        .context("client selection must be a number")?;
    if index == 0 || index > detected.len() {
        bail!("client selection is out of range");
    }
    Ok(index - 1)
}

fn interactive_prompt_allowed() -> bool {
    env::var("AMAI_FORCE_INTERACTIVE_PROMPT").unwrap_or_default() == "1"
        || (io::stdin().is_terminal() && io::stdout().is_terminal())
}

pub(crate) fn describe_client_surface(
    repo_root: &Path,
    requested_client: Option<&str>,
) -> Result<Value> {
    let resolution = resolve_client_target(repo_root, requested_client.unwrap_or("auto"), false)?;
    let config_output_path = resolve_output_path(repo_root, &resolution.target, None)?;
    let startup = resolution.target.startup_instructions.as_ref();
    let home = dirs::home_dir().ok_or_else(|| anyhow!("failed to resolve user home directory"))?;
    let startup_instruction_path =
        startup.map(|startup| expand_target_template(&startup.default_output, repo_root, &home));
    let openclaw_agent_id =
        (resolution.client_key == "openclaw").then(|| openclaw_agent_id(repo_root));
    let openclaw_agent_workspace = openclaw_agent_id
        .as_ref()
        .map(|_| repo_root.join(".openclaw").display().to_string());
    let reconnect_shell_command = format!(
        "./scripts/reconnect_local.sh --client {}",
        resolution.client_key
    );
    let reconnect_bootstrap_command = format!(
        "./scripts/amai_exec.sh bootstrap reconnect --client {} --yes",
        resolution.client_key
    );
    let delivery_surface_assist_summary = format!(
        "Для front-door новой чистой рабочей поверхности используй {} или {}.",
        reconnect_shell_command, reconnect_bootstrap_command
    );
    Ok(json!({
        "client_key": resolution.client_key,
        "display_name": resolution.target.display_name,
        "auto_selected": resolution.auto_selected,
        "selection_reason": resolution.reason,
        "other_detected_clients": resolution.other_detected_clients,
        "config_output_path": config_output_path.display().to_string(),
        "config_install_scope": resolution.target.install_scope,
        "config_install_scope_label": install_scope_status(&resolution.target.install_scope),
        "startup_instruction_mode": startup.map(|item| item.mode.clone()),
        "startup_instruction_format": startup.map(|item| item.format.clone()),
        "startup_instruction_install_scope": startup.map(|item| item.install_scope.clone()),
        "startup_instruction_install_scope_label": startup.map(|item| install_scope_status(&item.install_scope)),
        "startup_instruction_path": startup_instruction_path.map(|path| path.display().to_string()),
        "client_runtime_agent_id": openclaw_agent_id,
        "client_runtime_workspace_path": openclaw_agent_workspace,
        "reconnect_shell_command": reconnect_shell_command,
        "reconnect_bootstrap_command": reconnect_bootstrap_command,
        "fresh_chat_assist_summary": delivery_surface_assist_summary,
        "delivery_surface_assist_summary": delivery_surface_assist_summary,
    }))
}

fn resolve_output_path(
    repo_root: &Path,
    target: &ClientTarget,
    explicit: Option<&PathBuf>,
) -> Result<PathBuf> {
    if let Some(path) = explicit {
        let resolved = if path.is_absolute() {
            path.clone()
        } else {
            repo_root.join(path)
        };
        return Ok(resolved);
    }

    let home = dirs::home_dir().ok_or_else(|| anyhow!("failed to resolve user home directory"))?;
    Ok(expand_target_template(
        &target.default_output,
        repo_root,
        &home,
    ))
}

fn expand_target_template(template: &str, repo_root: &Path, home: &Path) -> PathBuf {
    PathBuf::from(
        template
            .replace("${repo_root}", &repo_root.display().to_string())
            .replace("${home}", &home.display().to_string()),
    )
}

const STARTUP_INSTRUCTIONS_MARKER: &str = "<!-- AMAI MANAGED STARTUP INSTRUCTIONS v1 -->";
const STARTUP_INSTRUCTIONS_END_MARKER: &str = "<!-- /AMAI MANAGED STARTUP INSTRUCTIONS v1 -->";

fn startup_contract_artifact_path(repo_root: &Path) -> PathBuf {
    repo_root.join(".amai/onboarding/project-chat-startup-contract.json")
}

fn startup_agent_contract_artifact_path(repo_root: &Path) -> PathBuf {
    repo_root.join(".amai/onboarding/project-chat-startup-agent-contract.json")
}

fn agent_preflight_contract_artifact_path(repo_root: &Path) -> PathBuf {
    repo_root.join(".amai/onboarding/project-agent-preflight-contract.json")
}

fn agent_preflight_agent_contract_artifact_path(repo_root: &Path) -> PathBuf {
    repo_root.join(".amai/onboarding/project-agent-preflight-agent-contract.json")
}

fn agent_preflight_state_artifact_path(repo_root: &Path) -> PathBuf {
    repo_root.join(".amai/onboarding/project-agent-preflight-state.json")
}

fn managed_startup_block_bounds(content: &str) -> Result<Option<(usize, usize)>> {
    let start = content.find(STARTUP_INSTRUCTIONS_MARKER);
    let end = content.find(STARTUP_INSTRUCTIONS_END_MARKER);
    match (start, end) {
        (None, None) => Ok(None),
        (Some(_), None) | (None, Some(_)) => Err(anyhow!(
            "managed startup block is malformed: expected both start and end markers"
        )),
        (Some(start_index), Some(end_index)) => {
            if end_index < start_index {
                return Err(anyhow!(
                    "managed startup block is malformed: end marker precedes start marker"
                ));
            }
            Ok(Some((
                start_index,
                end_index + STARTUP_INSTRUCTIONS_END_MARKER.len(),
            )))
        }
    }
}

fn merge_managed_startup_block(existing: &str, block: &str) -> Result<String> {
    let block = block.trim();
    if block.is_empty() {
        bail!("managed startup block must not be empty");
    }
    if let Some((start, end)) = managed_startup_block_bounds(existing)? {
        let prefix = existing[..start].trim_end();
        let suffix = existing[end..].trim_start();
        return Ok(match (prefix.is_empty(), suffix.is_empty()) {
            (true, true) => format!("{block}\n"),
            (false, true) => format!("{prefix}\n\n{block}\n"),
            (true, false) => format!("{block}\n\n{suffix}\n"),
            (false, false) => format!("{prefix}\n\n{block}\n\n{suffix}\n"),
        });
    }
    if existing.trim().is_empty() {
        Ok(format!("{block}\n"))
    } else {
        Ok(format!("{}\n\n{block}\n", existing.trim_end()))
    }
}

fn strip_managed_startup_block(existing: &str) -> Result<Option<String>> {
    let Some((start, end)) = managed_startup_block_bounds(existing)? else {
        return Ok(None);
    };
    let prefix = existing[..start].trim_end();
    let suffix = existing[end..].trim_start();
    let stripped = match (prefix.is_empty(), suffix.is_empty()) {
        (true, true) => String::new(),
        (false, true) => format!("{prefix}\n"),
        (true, false) => format!("{suffix}\n"),
        (false, false) => format!("{prefix}\n\n{suffix}\n"),
    };
    Ok(Some(stripped))
}

fn install_startup_instructions(
    workspace_root: &Path,
    helper_repo_root: &Path,
    client_resolution: &ClientResolution,
) -> Result<Option<StartupInstructionsInstallSummary>> {
    let Some(startup) = &client_resolution.target.startup_instructions else {
        return Ok(None);
    };
    let home = dirs::home_dir().ok_or_else(|| anyhow!("failed to resolve user home directory"))?;
    let output_path = expand_target_template(&startup.default_output, workspace_root, &home);
    let content = render_startup_instructions(
        workspace_root,
        helper_repo_root,
        &client_resolution.target.display_name,
        &client_resolution.client_key,
        &startup.format,
    )?;

    match startup.mode.as_str() {
        "managed_workspace_file" => {
            if let Some(parent) = output_path.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("failed to create {}", parent.display()))?;
            }
            if output_path.is_file() {
                let existing = fs::read_to_string(&output_path)
                    .with_context(|| format!("failed to read {}", output_path.display()))?;
                if !existing.contains(STARTUP_INSTRUCTIONS_MARKER)
                    && existing.trim() != content.trim()
                {
                    let fallback = workspace_root.join("tmp/onboarding").join(format!(
                        "{}-amai-startup-manual.md",
                        client_resolution.client_key
                    ));
                    if let Some(parent) = fallback.parent() {
                        fs::create_dir_all(parent)
                            .with_context(|| format!("failed to create {}", parent.display()))?;
                    }
                    fs::write(&fallback, content.as_bytes())
                        .with_context(|| format!("failed to write {}", fallback.display()))?;
                    return Ok(Some(StartupInstructionsInstallSummary {
                        status: "managed_target_conflict_manual_snippet_generated".to_string(),
                        output_path: fallback,
                        install_scope: "manual_generated".to_string(),
                        auto_start_ready: false,
                        reason: format!(
                            "existing unmanaged startup instruction file left in place: {}",
                            output_path.display()
                        ),
                    }));
                }
            }
            fs::write(&output_path, content.as_bytes())
                .with_context(|| format!("failed to write {}", output_path.display()))?;
            Ok(Some(StartupInstructionsInstallSummary {
                status: "managed_workspace_instruction_installed".to_string(),
                output_path,
                install_scope: startup.install_scope.clone(),
                auto_start_ready: true,
                reason: "client-native startup instructions are now installed alongside MCP config"
                    .to_string(),
            }))
        }
        "managed_append_block" => {
            if let Some(parent) = output_path.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("failed to create {}", parent.display()))?;
            }
            let existing = if output_path.is_file() {
                fs::read_to_string(&output_path)
                    .with_context(|| format!("failed to read {}", output_path.display()))?
            } else {
                String::new()
            };
            let merged = merge_managed_startup_block(&existing, &content)?;
            fs::write(&output_path, merged.as_bytes())
                .with_context(|| format!("failed to write {}", output_path.display()))?;
            Ok(Some(StartupInstructionsInstallSummary {
                status: "managed_append_instruction_installed".to_string(),
                output_path,
                install_scope: startup.install_scope.clone(),
                auto_start_ready: true,
                reason: "client-native startup block is now embedded in the project rule file"
                    .to_string(),
            }))
        }
        "manual_snippet_only" => {
            if let Some(parent) = output_path.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("failed to create {}", parent.display()))?;
            }
            fs::write(&output_path, content.as_bytes())
                .with_context(|| format!("failed to write {}", output_path.display()))?;
            Ok(Some(StartupInstructionsInstallSummary {
                status: "manual_startup_snippet_generated".to_string(),
                output_path,
                install_scope: startup.install_scope.clone(),
                auto_start_ready: false,
                reason:
                    "this client still needs an explicit project instruction/rule integration path"
                        .to_string(),
            }))
        }
        other => Err(anyhow!(
            "unsupported startup_instructions.mode in config/client_targets.toml: {other}"
        )),
    }
}

fn install_client_runtime_artifacts(
    repo_root: &Path,
    client_config_path: &Path,
    client_resolution: &ClientResolution,
    startup_summary: Option<&mut StartupInstructionsInstallSummary>,
) -> Result<Option<ClientRuntimeInstallSummary>> {
    if client_resolution.client_key == "hermes" {
        let Some(startup_summary) = startup_summary else {
            return Ok(None);
        };
        if startup_summary.status != "managed_workspace_instruction_installed" {
            return Ok(None);
        }
        let summary = ensure_hermes_project_profile(
            repo_root,
            client_config_path,
            &startup_summary.output_path,
        )?;
        startup_summary.status = "managed_hermes_profile_installed".to_string();
        startup_summary.auto_start_ready = true;
        startup_summary.reason =
            "dedicated Hermes profile is now the sticky default and carries repo-bound Amai startup automatically".to_string();
        return Ok(Some(ClientRuntimeInstallSummary {
            status: "managed_hermes_profile_registered".to_string(),
            output_path: summary.profile_dir,
            install_scope: "user_global".to_string(),
            reason: format!(
                "Hermes profile `{}` is now the sticky default and boots with repo-bound Amai startup even outside the repo cwd",
                summary.profile_name
            ),
        }));
    }

    if client_resolution.client_key != "openclaw" {
        return Ok(None);
    }
    let Some(startup_summary) = startup_summary else {
        return Ok(None);
    };
    if startup_summary.status != "managed_workspace_instruction_installed" {
        return Ok(None);
    }
    let workspace_root = startup_summary
        .output_path
        .parent()
        .ok_or_else(|| anyhow!("OpenClaw startup instruction path has no parent workspace"))?;
    let workspace_root = workspace_root.to_path_buf();
    fs::create_dir_all(&workspace_root)
        .with_context(|| format!("failed to create {}", workspace_root.display()))?;
    let agent_id = openclaw_agent_id(repo_root);
    let agent = ensure_openclaw_project_agent(client_config_path, &agent_id, &workspace_root)?;
    startup_summary.status = "managed_openclaw_agent_workspace_installed".to_string();
    startup_summary.auto_start_ready = true;
    startup_summary.reason =
        "dedicated OpenClaw agent now points at the repo-local managed workspace".to_string();
    Ok(Some(ClientRuntimeInstallSummary {
        status: "managed_openclaw_agent_registered".to_string(),
        output_path: workspace_root,
        install_scope: "workspace_local".to_string(),
        reason: format!(
            "OpenClaw agent `{}` is registered against the repo-local workspace",
            agent.agent_id
        ),
    }))
}

fn install_startup_contract_artifact(
    workspace_root: &Path,
    helper_repo_root: &Path,
) -> Result<Option<StartupContractInstallSummary>> {
    let output_path = startup_contract_artifact_path(workspace_root);
    let agent_output_path = startup_agent_contract_artifact_path(workspace_root);
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let (content, sha256) = render_startup_contract_artifact(workspace_root, helper_repo_root)?;
    let agent_content = render_startup_agent_contract_artifact(workspace_root, helper_repo_root)?;
    fs::write(&output_path, content.as_bytes())
        .with_context(|| format!("failed to write {}", output_path.display()))?;
    fs::write(&agent_output_path, agent_content.as_bytes())
        .with_context(|| format!("failed to write {}", agent_output_path.display()))?;
    Ok(Some(StartupContractInstallSummary {
        status: "workspace_startup_contract_materialized".to_string(),
        output_path,
        install_scope: "workspace_local".to_string(),
        reason:
            "supported clients now get a machine-readable startup source-of-truth alongside managed instructions"
                .to_string(),
        sha256,
    }))
}

#[derive(Debug, Clone, Copy)]
struct AgentPreflightDocumentDescriptor {
    order: usize,
    path: &'static str,
    role: &'static str,
    condition: &'static str,
    tree_location: &'static str,
}

const AGENT_PREFLIGHT_DOCUMENTS: &[AgentPreflightDocumentDescriptor] = &[
    AgentPreflightDocumentDescriptor {
        order: 1,
        path: "AGENTS.md",
        role: "обязательный runtime/startup law и главный проектный контракт",
        condition: "always",
        tree_location: "root",
    },
    AgentPreflightDocumentDescriptor {
        order: 2,
        path: "README.md",
        role: "продуктовая картина и базовый старт",
        condition: "always",
        tree_location: "root",
    },
    AgentPreflightDocumentDescriptor {
        order: 3,
        path: "docs/AGENT_START_HERE.md",
        role: "decision tree и быстрый вход в проект",
        condition: "always",
        tree_location: "root",
    },
    AgentPreflightDocumentDescriptor {
        order: 4,
        path: "docs/IMPLEMENTATION_STATUS.md",
        role: "живой status snapshot, checkbox-chain и ближайший этап",
        condition: "always",
        tree_location: "trunk",
    },
    AgentPreflightDocumentDescriptor {
        order: 5,
        path: "docs/ARCHITECTURE.md",
        role: "текущий materialized baseline",
        condition: "always",
        tree_location: "current_state_branch",
    },
    AgentPreflightDocumentDescriptor {
        order: 6,
        path: "docs/OPERATIONS.md",
        role: "proof, fail-closed и operational laws",
        condition: "always",
        tree_location: "current_state_branch",
    },
    AgentPreflightDocumentDescriptor {
        order: 7,
        path: "docs/AMAI_GLOBAL_MEMORY_ROADMAP.md",
        role: "канонический target-state roadmap",
        condition: "always",
        tree_location: "target_state_branch",
    },
    AgentPreflightDocumentDescriptor {
        order: 8,
        path: "docs/IMPLEMENTATION_GATES.md",
        role: "proof/debug/reconcile карта по этапам",
        condition: "implementation_stage_only",
        tree_location: "target_state_branch",
    },
    AgentPreflightDocumentDescriptor {
        order: 9,
        path: "docs/AMAI_TASK_TREE_PLAN.md",
        role: "частный модульный план task/commitment graph",
        condition: "task_graph_or_memory_module_only",
        tree_location: "module_branch",
    },
    AgentPreflightDocumentDescriptor {
        order: 10,
        path: "docs/AMAI_COMPARE_EXPERIMENT_PLAN.md",
        role: "частный модульный план compare/eval surface",
        condition: "compare_or_eval_module_only",
        tree_location: "module_branch",
    },
];

fn project_supports_agent_preflight(workspace_root: &Path) -> bool {
    AGENT_PREFLIGHT_DOCUMENTS
        .iter()
        .all(|doc| workspace_root.join(doc.path).is_file())
}

fn value_sha256(value: &Value) -> Result<String> {
    let bytes = serde_json::to_vec(value).context("failed to serialize json value for sha256")?;
    Ok(hex_sha256(&bytes))
}

fn agent_preflight_required_documents_json() -> Vec<Value> {
    AGENT_PREFLIGHT_DOCUMENTS
        .iter()
        .map(|doc| {
            json!({
                "order": doc.order,
                "path": doc.path,
                "role": doc.role,
                "condition": doc.condition,
                "tree_location": doc.tree_location
            })
        })
        .collect()
}

fn agent_preflight_required_document_snapshots(workspace_root: &Path) -> Result<Vec<Value>> {
    AGENT_PREFLIGHT_DOCUMENTS
        .iter()
        .map(|doc| {
            let path = workspace_root.join(doc.path);
            let exists = path.is_file();
            let sha256 = if exists {
                Some(file_sha256(&path)?)
            } else {
                None
            };
            let modified = if exists {
                Some(file_modified_epoch_seconds(&path)?)
            } else {
                None
            };
            Ok(json!({
                "order": doc.order,
                "path": doc.path,
                "absolute_path": path.display().to_string(),
                "role": doc.role,
                "condition": doc.condition,
                "tree_location": doc.tree_location,
                "exists": exists,
                "sha256": sha256,
                "last_modified_epoch_seconds": modified
            }))
        })
        .collect()
}

fn file_sha256(path: &Path) -> Result<String> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    Ok(hex_sha256(&bytes))
}

fn file_modified_epoch_seconds(path: &Path) -> Result<u64> {
    let modified = fs::metadata(path)
        .with_context(|| format!("failed to stat {}", path.display()))?
        .modified()
        .with_context(|| format!("failed to read mtime for {}", path.display()))?;
    let duration = modified
        .duration_since(UNIX_EPOCH)
        .with_context(|| format!("mtime for {} predates epoch", path.display()))?;
    Ok(duration.as_secs())
}

fn markdown_section_lines<'a>(content: &'a str, heading: &str) -> Result<Vec<&'a str>> {
    let mut found = false;
    let mut lines = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if !found {
            if trimmed == heading {
                found = true;
            }
            continue;
        }
        if trimmed.starts_with("## ") || trimmed.starts_with("### ") {
            break;
        }
        lines.push(line);
    }
    if !found {
        bail!("required markdown heading not found: {heading}");
    }
    Ok(lines)
}

fn markdown_bullets_under_heading(content: &str, heading: &str) -> Result<Vec<String>> {
    Ok(markdown_section_lines(content, heading)?
        .into_iter()
        .filter_map(|line| line.trim().strip_prefix("- ").map(str::to_owned))
        .collect())
}

fn markdown_items_under_heading(content: &str, heading: &str) -> Result<Vec<String>> {
    Ok(markdown_section_lines(content, heading)?
        .into_iter()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                None
            } else if let Some(bullet) = trimmed.strip_prefix("- ") {
                Some(bullet.to_string())
            } else {
                Some(trimmed.to_string())
            }
        })
        .collect())
}

fn parse_markdown_link(input: &str) -> Result<(String, String)> {
    let trimmed = input.trim();
    let Some(stripped) = trimmed.strip_prefix('[') else {
        bail!("expected markdown link, got: {trimmed}");
    };
    let Some((label, rest)) = stripped.split_once("](") else {
        bail!("expected markdown link separator in: {trimmed}");
    };
    let Some(target) = rest.strip_suffix(')') else {
        bail!("expected markdown link closing paren in: {trimmed}");
    };
    Ok((label.to_string(), target.to_string()))
}

fn parse_stage_checklist(content: &str) -> Result<Vec<Value>> {
    markdown_section_lines(content, "## Чеклист этапов")?
        .into_iter()
        .filter(|line| line.trim().starts_with("- ["))
        .map(|line| {
            let trimmed = line.trim();
            let (checked, remainder) = if let Some(rest) = trimmed.strip_prefix("- [x] ") {
                (true, rest)
            } else if let Some(rest) = trimmed.strip_prefix("- [ ] ") {
                (false, rest)
            } else {
                bail!("unsupported checkbox line format: {trimmed}");
            };
            let (label, roadmap_path) = parse_markdown_link(remainder)?;
            Ok(json!({
                "checked": checked,
                "label": label,
                "roadmap_path": roadmap_path
            }))
        })
        .collect()
}

fn first_backticked_span(input: &str) -> Option<String> {
    let start = input.find('`')?;
    let rest = &input[start + 1..];
    let end = rest.find('`')?;
    Some(rest[..end].to_string())
}

fn parse_declared_next_stage_label(content: &str) -> Result<Option<String>> {
    for item in markdown_items_under_heading(content, "### Ближайший следующий этап")?
    {
        if let Some(label) = first_backticked_span(&item) {
            return Ok(Some(label));
        }
    }
    Ok(None)
}

fn helper_script_command(
    workspace_root: &Path,
    helper_repo_root: &Path,
    script_name: &str,
) -> String {
    if workspace_root == helper_repo_root {
        format!("./scripts/{script_name}")
    } else {
        helper_repo_root
            .join("scripts")
            .join(script_name)
            .display()
            .to_string()
    }
}

fn agent_preflight_contract_for_workspace(
    workspace_root: &Path,
    helper_repo_root: &Path,
) -> Result<(Value, String)> {
    if !project_supports_agent_preflight(workspace_root) {
        bail!(
            "workspace {} does not contain the required Amai agent-preflight docs",
            workspace_root.display()
        );
    }
    let contract = json!({
        "contract_version": "agent-preflight-contract-v1",
        "purpose": "machine-readable onboarding, status snapshot, and stage-discipline front door for any agent before touching code, schema, or runtime in this workspace",
        "required_documents": agent_preflight_required_documents_json(),
        "document_tree": {
            "root_documents": [
                "AGENTS.md",
                "README.md",
                "docs/AGENT_START_HERE.md",
                "docs/IMPLEMENTATION_STATUS.md"
            ],
            "trunk_document": "docs/IMPLEMENTATION_STATUS.md",
            "branches": [
                {
                    "branch_kind": "current_state_or_bugfix",
                    "when": "task is about current implementation baseline or bugfix",
                    "required_documents": ["docs/ARCHITECTURE.md", "docs/OPERATIONS.md"]
                },
                {
                    "branch_kind": "implementation_stage",
                    "when": "task is about the next roadmap stage or stage-based implementation",
                    "required_documents": ["docs/AMAI_GLOBAL_MEMORY_ROADMAP.md", "docs/IMPLEMENTATION_GATES.md"]
                },
                {
                    "branch_kind": "module_work",
                    "when": "task touches task graph, compare/eval, or another memory module",
                    "required_documents": ["docs/AMAI_TASK_TREE_PLAN.md", "docs/AMAI_COMPARE_EXPERIMENT_PLAN.md"]
                }
            ]
        },
        "stage_discipline": {
            "must_read_agents_md_first": true,
            "must_open_status_before_code": true,
            "must_follow_checkbox_order": true,
            "must_open_matching_stage_gate_document": true,
            "must_use_existing_benchmark_or_proof_harness_before_adhoc_checks": true,
            "checkbox_requires_tests_manual_check_debug_fix_retest": true,
            "must_update_implementation_status_after_significant_step": true,
            "must_write_continuity_handoff_after_significant_step": true,
            "missing_or_unreadable_fail_closed": true
        },
        "status_sources": {
            "status_snapshot_path": "docs/IMPLEMENTATION_STATUS.md",
            "agent_entry_path": "docs/AGENT_START_HERE.md",
            "roadmap_path": "docs/AMAI_GLOBAL_MEMORY_ROADMAP.md",
            "gates_path": "docs/IMPLEMENTATION_GATES.md"
        },
        "refresh_commands": {
            "cli_command": "bootstrap agent-preflight",
            "shell_command": helper_script_command(workspace_root, helper_repo_root, "agent_preflight.sh"),
            "json_flag": "--json"
        },
        "runtime_state_artifact": {
            "workspace_runtime_state_relative_path": ".amai/onboarding/project-agent-preflight-state.json",
            "workspace_runtime_state_artifact_version": "workspace-agent-preflight-state-v1",
            "source_summary_field": "agent_preflight_summary",
            "written_by_tool": "amai bootstrap agent-preflight"
        },
        "fail_closed_conditions": {
            "missing_or_unreadable_required_documents": true,
            "missing_or_unreadable_status_snapshot": true,
            "next_stage_drift_detected": true
        }
    });
    let sha256 = value_sha256(&contract)?;
    Ok((contract, sha256))
}

fn render_agent_preflight_contract_artifact(
    workspace_root: &Path,
    helper_repo_root: &Path,
) -> Result<(String, String)> {
    let (contract, sha256) =
        agent_preflight_contract_for_workspace(workspace_root, helper_repo_root)?;
    let payload = json!({
        "artifact_version": "workspace-agent-preflight-contract-v1",
        "contract_kind": "project_agent_preflight",
        "repo_root": workspace_root.display().to_string(),
        "preflight_contract_sha256": sha256,
        "preflight_contract_sha256_scope": "preflight_contract object only",
        "preflight_contract": contract
    });
    Ok((
        serde_json::to_string(&payload)
            .context("failed to serialize agent preflight contract artifact")?,
        payload["preflight_contract_sha256"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
    ))
}

fn render_agent_preflight_agent_contract_artifact(
    workspace_root: &Path,
    helper_repo_root: &Path,
) -> Result<String> {
    let (contract, sha256) =
        agent_preflight_contract_for_workspace(workspace_root, helper_repo_root)?;
    let payload = json!({
        "artifact_version": "workspace-agent-preflight-agent-contract-v1",
        "contract_kind": "project_agent_preflight_agent_read",
        "repo_root": workspace_root.display().to_string(),
        "full_preflight_contract_relative_path": ".amai/onboarding/project-agent-preflight-contract.json",
        "full_preflight_contract_sha256": sha256,
        "required_start_order": contract["required_documents"].clone(),
        "document_tree": contract["document_tree"].clone(),
        "stage_discipline": contract["stage_discipline"].clone(),
        "refresh_commands": contract["refresh_commands"].clone(),
        "status_sources": contract["status_sources"].clone(),
        "runtime_state_artifact": contract["runtime_state_artifact"].clone()
    });
    serde_json::to_string(&payload)
        .context("failed to serialize compact agent preflight contract artifact")
}

fn render_agent_preflight_state_artifact(
    workspace_root: &Path,
    helper_repo_root: &Path,
) -> Result<String> {
    let (contract, _) = agent_preflight_contract_for_workspace(workspace_root, helper_repo_root)?;
    let status_path = workspace_root.join("docs/IMPLEMENTATION_STATUS.md");
    let status_text = fs::read_to_string(&status_path)
        .with_context(|| format!("failed to read {}", status_path.display()))?;
    let gates_path = workspace_root.join("docs/IMPLEMENTATION_GATES.md");
    if !gates_path.is_file() {
        bail!(
            "required implementation gates doc is missing: {}",
            gates_path.display()
        );
    }

    let stage_checklist = parse_stage_checklist(&status_text)?;
    let next_required_stage = stage_checklist
        .iter()
        .find(|item| !item["checked"].as_bool().unwrap_or(false))
        .cloned();
    let declared_next_stage = parse_declared_next_stage_label(&status_text)?;
    if let (Some(expected), Some(declared)) = (
        next_required_stage
            .as_ref()
            .and_then(|value| value["label"].as_str())
            .map(str::to_owned),
        declared_next_stage.clone(),
    ) {
        if expected != declared {
            bail!(
                "implementation status drift detected: first unchecked stage is `{expected}`, but declared next stage is `{declared}`"
            );
        }
    }

    let next_stage_ready_mechanisms = if let Some(stage) = &next_required_stage {
        let heading = format!(
            "### {}",
            stage["label"]
                .as_str()
                .ok_or_else(|| anyhow!("next stage label missing from checklist"))?
        );
        markdown_items_under_heading(&status_text, &heading)?
    } else {
        Vec::new()
    };
    let stage_progress_state = if next_required_stage.is_some() {
        "next_stage_required"
    } else {
        "all_stages_closed"
    };

    let payload = json!({
        "artifact_version": "workspace-agent-preflight-state-v1",
        "contract_kind": "project_agent_preflight_state",
        "repo_root": workspace_root.display().to_string(),
        "source_documents": agent_preflight_required_document_snapshots(workspace_root)?,
        "agent_preflight_summary": {
            "status_snapshot_path": "docs/IMPLEMENTATION_STATUS.md",
            "status_snapshot_sha256": file_sha256(&status_path)?,
            "overall_state": markdown_bullets_under_heading(&status_text, "### Общая оценка")?,
            "materialized_baseline": markdown_bullets_under_heading(&status_text, "### Что уже точно сделано")?,
            "design_closed": markdown_bullets_under_heading(&status_text, "### Что уже закрыто на уровне дизайна")?,
            "not_materialized": markdown_bullets_under_heading(&status_text, "### Что ещё не materialized в коде")?,
            "active_focus": markdown_bullets_under_heading(&status_text, "### Что сейчас в работе")?,
            "blockers": markdown_bullets_under_heading(&status_text, "### Фундаментальные blocker-ы")?,
            "stage_checklist": stage_checklist,
            "declared_next_stage_label": declared_next_stage,
            "stage_progress_state": stage_progress_state,
            "next_required_stage": next_required_stage,
            "next_stage_ready_mechanisms": next_stage_ready_mechanisms,
            "ready_harnesses_source_path": "docs/IMPLEMENTATION_STATUS.md",
            "gates_document_path": "docs/IMPLEMENTATION_GATES.md",
            "roadmap_path": "docs/AMAI_GLOBAL_MEMORY_ROADMAP.md"
        },
        "preflight_execution_gate": contract["stage_discipline"].clone(),
        "refresh_commands": contract["refresh_commands"].clone()
    });
    serde_json::to_string(&payload).context("failed to serialize agent preflight state artifact")
}

fn install_agent_preflight_artifacts(
    workspace_root: &Path,
    helper_repo_root: &Path,
) -> Result<Option<AgentPreflightInstallSummary>> {
    if !project_supports_agent_preflight(workspace_root) {
        return Ok(None);
    }
    let contract_output_path = agent_preflight_contract_artifact_path(workspace_root);
    let agent_output_path = agent_preflight_agent_contract_artifact_path(workspace_root);
    let state_output_path = agent_preflight_state_artifact_path(workspace_root);
    if let Some(parent) = contract_output_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let (contract_text, sha256) =
        render_agent_preflight_contract_artifact(workspace_root, helper_repo_root)?;
    let agent_text =
        render_agent_preflight_agent_contract_artifact(workspace_root, helper_repo_root)?;
    let state_text = render_agent_preflight_state_artifact(workspace_root, helper_repo_root)?;
    fs::write(&contract_output_path, contract_text.as_bytes())
        .with_context(|| format!("failed to write {}", contract_output_path.display()))?;
    fs::write(&agent_output_path, agent_text.as_bytes())
        .with_context(|| format!("failed to write {}", agent_output_path.display()))?;
    fs::write(&state_output_path, state_text.as_bytes())
        .with_context(|| format!("failed to write {}", state_output_path.display()))?;
    Ok(Some(AgentPreflightInstallSummary {
        status: "workspace_agent_preflight_materialized".to_string(),
        contract_output_path,
        agent_output_path,
        state_output_path,
        install_scope: "workspace_local".to_string(),
        reason:
            "supported project workspaces now get a machine-readable agent preflight gate alongside startup artifacts"
                .to_string(),
        sha256,
        refresh_shell_command: helper_script_command(workspace_root, helper_repo_root, "agent_preflight.sh"),
    }))
}

fn remove_startup_instructions(
    repo_root: &Path,
    target: &ClientTarget,
) -> Result<Option<StartupInstructionsInstallSummary>> {
    let Some(startup) = &target.startup_instructions else {
        return Ok(None);
    };
    let home = dirs::home_dir().ok_or_else(|| anyhow!("failed to resolve user home directory"))?;
    let output_path = expand_target_template(&startup.default_output, repo_root, &home);
    if !output_path.is_file() {
        return Ok(None);
    }

    let existing = fs::read_to_string(&output_path)
        .with_context(|| format!("failed to read {}", output_path.display()))?;
    match startup.mode.as_str() {
        "managed_workspace_file" | "manual_snippet_only" => {
            let removable = startup.mode == "manual_snippet_only"
                || existing.contains(STARTUP_INSTRUCTIONS_MARKER);
            if !removable {
                return Ok(None);
            }

            fs::remove_file(&output_path)
                .with_context(|| format!("failed to remove {}", output_path.display()))?;
            Ok(Some(StartupInstructionsInstallSummary {
                status: "startup_instructions_removed".to_string(),
                output_path,
                install_scope: startup.install_scope.clone(),
                auto_start_ready: false,
                reason: "Amai-managed startup instructions removed".to_string(),
            }))
        }
        "managed_append_block" => {
            let Some(stripped) = strip_managed_startup_block(&existing)? else {
                return Ok(None);
            };
            if stripped.trim().is_empty() {
                fs::remove_file(&output_path)
                    .with_context(|| format!("failed to remove {}", output_path.display()))?;
            } else {
                fs::write(&output_path, stripped.as_bytes())
                    .with_context(|| format!("failed to write {}", output_path.display()))?;
            }
            Ok(Some(StartupInstructionsInstallSummary {
                status: "startup_instructions_removed".to_string(),
                output_path,
                install_scope: startup.install_scope.clone(),
                auto_start_ready: false,
                reason: "Amai-managed startup block removed".to_string(),
            }))
        }
        other => Err(anyhow!(
            "unsupported startup_instructions.mode in config/client_targets.toml: {other}"
        )),
    }
}

fn remove_client_runtime_artifacts(
    repo_root: &Path,
    client_key: &str,
    client_config_path: &Path,
) -> Result<Option<ClientRuntimeInstallSummary>> {
    if client_key == "hermes" {
        return remove_hermes_project_profile(repo_root);
    }
    if client_key != "openclaw" {
        return Ok(None);
    }
    let agent_id = openclaw_agent_id(repo_root);
    if !openclaw_agent_exists(client_config_path, &agent_id)? {
        return Ok(None);
    }
    delete_openclaw_project_agent(client_config_path, &agent_id)?;
    Ok(Some(ClientRuntimeInstallSummary {
        status: "openclaw_agent_removed".to_string(),
        output_path: repo_root.join(".openclaw"),
        install_scope: "workspace_local".to_string(),
        reason: format!("OpenClaw agent `{agent_id}` removed from user config"),
    }))
}

fn render_startup_instructions(
    workspace_root: &Path,
    helper_repo_root: &Path,
    client_display_name: &str,
    client_key: &str,
    format: &str,
) -> Result<String> {
    let body = match format {
        "hermes_compact_markdown" => {
            render_hermes_compact_startup_body(workspace_root, helper_repo_root, client_key)?
        }
        _ => render_startup_instruction_body(workspace_root, helper_repo_root, client_key)?,
    };
    match format {
        "vscode_instructions_md" => Ok(format!(
            "{STARTUP_INSTRUCTIONS_MARKER}\n---\napplyTo: \"**\"\ndescription: \"Amai continuity startup for this workspace\"\n---\n\n# Amai continuity startup ({client_display_name})\n\n{body}\n{STARTUP_INSTRUCTIONS_END_MARKER}\n"
        )),
        "cursor_rules_mdc" => Ok(format!(
            "{STARTUP_INSTRUCTIONS_MARKER}\n---\ndescription: Amai continuity startup for this workspace\nglobs: [\"**/*\"]\nalwaysApply: true\n---\n\n# Amai continuity startup ({client_display_name})\n\n{body}\n{STARTUP_INSTRUCTIONS_END_MARKER}\n"
        )),
        "codex_agents_snippet" => Ok(format!(
            "{STARTUP_INSTRUCTIONS_MARKER}\n# Amai continuity startup for Codex\n\nЭтот managed block должен жить в project `AGENTS.md`, а не в global config.\n\n{body}\n{STARTUP_INSTRUCTIONS_END_MARKER}\n"
        )),
        "hermes_compact_markdown" => Ok(format!(
            "{STARTUP_INSTRUCTIONS_MARKER}\n# Amai continuity startup ({client_display_name})\n\nЭтот managed startup должен оставаться compact contract-pointer, а не копией полного startup-law. Machine-readable startup contract остаётся source-of-truth.\n\n{body}\n{STARTUP_INSTRUCTIONS_END_MARKER}\n"
        )),
        "generic_markdown" => Ok(format!(
            "{STARTUP_INSTRUCTIONS_MARKER}\n# Amai continuity startup ({client_display_name})\n\n{body}\n{STARTUP_INSTRUCTIONS_END_MARKER}\n"
        )),
        other => Err(anyhow!(
            "unsupported startup instructions format for client {client_key}: {other}"
        )),
    }
}

fn render_hermes_compact_startup_body(
    workspace_root: &Path,
    helper_repo_root: &Path,
    client_key: &str,
) -> Result<String> {
    let (contract, startup_contract_sha256) =
        startup_contract_for_workspace(workspace_root, helper_repo_root)?;
    let tool = contract["tool"]
        .as_str()
        .ok_or_else(|| anyhow!("project_chat_startup contract is missing tool"))?;
    let namespace = contract["default_namespace"]
        .as_str()
        .ok_or_else(|| anyhow!("project_chat_startup contract is missing default_namespace"))?;
    let artifact_enforcement = &contract["artifact_enforcement"];
    let startup_contract_required_before_tool_call =
        artifact_enforcement["workspace_contract_required_before_tool_call"]
            .as_bool()
            .unwrap_or(false);
    let startup_contract_missing_or_unreadable_fail_closed =
        artifact_enforcement["missing_or_unreadable_fail_closed"]
            .as_bool()
            .unwrap_or(false);
    let startup_contract_sha256_mismatch_fail_closed =
        artifact_enforcement["sha256_mismatch_fail_closed"]
            .as_bool()
            .unwrap_or(false);
    let startup_contract_sha256_field = artifact_enforcement["workspace_contract_sha256_field"]
        .as_str()
        .unwrap_or("startup_contract_sha256");

    let tool_runtime_reconcile = &contract["tool_runtime_reconcile"];
    let reconcile_error_class = tool_runtime_reconcile["error_class"]
        .as_str()
        .unwrap_or("tool_execution_failed");
    let reconcile_error_detail_contains = tool_runtime_reconcile["error_detail_contains"]
        .as_str()
        .unwrap_or("no continuity import found for");
    let reconcile_local_cli_shell_command = tool_runtime_reconcile["local_cli"]["shell_command"]
        .as_str()
        .unwrap_or("./scripts/continuity_startup.sh");
    let reconcile_transport_error_detail_contains =
        tool_runtime_reconcile["transport_error_detail_contains"]
            .as_str()
            .unwrap_or("Transport closed");
    let reconnect_helper = &tool_runtime_reconcile["reconnect_helper"];
    let reconnect_helper_shell_relative_path = reconnect_helper["shell_helper_relative_path"]
        .as_str()
        .unwrap_or("./scripts/reconnect_local.sh");
    let reconnect_helper_bootstrap_command = reconnect_helper["bootstrap_command"]
        .as_str()
        .unwrap_or("bootstrap reconnect");
    let reconnect_helper_requires_client_argument = reconnect_helper["requires_client_argument"]
        .as_bool()
        .unwrap_or(false);
    let reconnect_helper_requires_yes_argument = reconnect_helper["requires_yes_argument"]
        .as_bool()
        .unwrap_or(false);
    let reconnect_shell_command = if helper_repo_root != workspace_root {
        let helper = helper_repo_root.display();
        if reconnect_helper_requires_client_argument {
            format!(
                "{reconnect_helper_shell_relative_path} --client {client_key} --cwd {} --workspace-root {} --yes",
                helper,
                workspace_root.display()
            )
        } else {
            format!(
                "{reconnect_helper_shell_relative_path} --cwd {} --workspace-root {} --yes",
                helper,
                workspace_root.display()
            )
        }
    } else if reconnect_helper_requires_client_argument {
        format!("{reconnect_helper_shell_relative_path} --client {client_key}")
    } else {
        reconnect_helper_shell_relative_path.to_string()
    };
    let reconnect_bootstrap_command = if helper_repo_root != workspace_root {
        let helper_exec = helper_repo_root.join("scripts/amai_exec.sh");
        if reconnect_helper_requires_client_argument {
            format!(
                "{} {} --client {client_key} --cwd {} --workspace-root {} --yes",
                helper_exec.display(),
                reconnect_helper_bootstrap_command,
                helper_repo_root.display(),
                workspace_root.display()
            )
        } else {
            format!(
                "{} {} --cwd {} --workspace-root {} --yes",
                helper_exec.display(),
                reconnect_helper_bootstrap_command,
                helper_repo_root.display(),
                workspace_root.display()
            )
        }
    } else if reconnect_helper_requires_client_argument {
        if reconnect_helper_requires_yes_argument {
            format!(
                "./scripts/amai_exec.sh {reconnect_helper_bootstrap_command} --client {client_key} --yes"
            )
        } else {
            format!(
                "./scripts/amai_exec.sh {reconnect_helper_bootstrap_command} --client {client_key}"
            )
        }
    } else if reconnect_helper_requires_yes_argument {
        format!("./scripts/amai_exec.sh {reconnect_helper_bootstrap_command} --yes")
    } else {
        format!("./scripts/amai_exec.sh {reconnect_helper_bootstrap_command}")
    };

    let runtime_state_artifact = &contract["runtime_state_artifact"];
    let runtime_state_relative_path =
        runtime_state_artifact["workspace_runtime_state_relative_path"]
            .as_str()
            .unwrap_or(".amai/continuity/project-chat-startup-state.json");
    let startup_execution_gate_field = runtime_state_artifact["startup_execution_gate_field"]
        .as_str()
        .unwrap_or("startup_execution_gate");
    let gate_semantics_consistent_field = runtime_state_artifact["gate_semantics_consistent_field"]
        .as_str()
        .unwrap_or("gate_semantics_consistent");
    let startup_state_fallback_shell_command =
        runtime_state_artifact["inspection_fallback_cli"]["shell_command"]
            .as_str()
            .unwrap_or("./scripts/continuity_startup_state.sh");

    let resume_enforcement = &contract["resume_enforcement"];
    let resume_state_field = resume_enforcement["resume_state_field"]
        .as_str()
        .unwrap_or("execctl_resume_state");
    let resume_contract_field = resume_enforcement["contract_field"]
        .as_str()
        .unwrap_or("execctl_resume_contract_summary");
    let resume_obligation_field = resume_enforcement["obligation_field"]
        .as_str()
        .unwrap_or("execctl_resume_obligation");
    let startup_next_action_field = resume_enforcement["startup_next_action_field"]
        .as_str()
        .unwrap_or("startup_next_action");
    let active_lease_field = resume_enforcement["active_lease_field"]
        .as_str()
        .unwrap_or("execctl_active_lease");
    let required_action_kind = resume_enforcement["required_action_kind_when_resume_required"]
        .as_str()
        .unwrap_or("resume_required_return_task");

    let live_budget = &contract["live_client_budget_enforcement"];
    let client_budget_guard_shell_command = live_budget["guard_shell_command"]
        .as_str()
        .unwrap_or("./scripts/client_budget_gate.sh");
    let client_budget_reply_execution_gate_field = live_budget["reply_execution_gate_field"]
        .as_str()
        .unwrap_or("reply_execution_gate");
    let client_budget_reply_prefix_field = live_budget["reply_prefix_field"]
        .as_str()
        .unwrap_or("reply_prefix");
    let client_budget_required_reply_prefix_source = live_budget["required_reply_prefix_source"]
        .as_str()
        .unwrap_or("personal_agent_online_limit_contour");
    let fail_closed_conditions = contract["fail_closed_conditions"]
        .as_array()
        .map(|values| {
            values
                .iter()
                .filter_map(|value| value.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();

    let repo_root_display = workspace_root.display().to_string();
    let agent_contract_path_display = startup_agent_contract_artifact_path(workspace_root)
        .display()
        .to_string();
    let contract_path_display = startup_contract_artifact_path(workspace_root)
        .display()
        .to_string();

    Ok(format!(
        "Перед первым содержательным ответом в новом или resumed чате и дальше перед каждым следующим содержательным ответом:\n\
1. Workspace = `{repo_root_display}`. Прочитай compact agent contract `{agent_contract_path_display}` и machine-readable startup contract `{contract_path_display}`. Startup contract остаётся pinned source-of-truth; до MCP tool call проверь `{startup_contract_sha256_field} = \"{startup_contract_sha256}\"`, `workspace_contract_required_before_tool_call = {startup_contract_required_before_tool_call}`, `missing_or_unreadable_fail_closed = {startup_contract_missing_or_unreadable_fail_closed}`, `sha256_mismatch_fail_closed = {startup_contract_sha256_mismatch_fail_closed}`.\n\
2. Затем вызови MCP tool `{tool}` с `repo_root = \"{repo_root_display}\"` и `namespace = \"{namespace}\"` до любого другого Amai шага.\n\
3. Если tool вернул `{reconcile_error_class}` с detail `{reconcile_error_detail_contains}` или transport error с detail `{reconcile_transport_error_detail_contains}`, немедленно делай local reconcile: `{reconcile_local_cli_shell_command} --repo-root \"{repo_root_display}\" --namespace \"{namespace}\" --json`. Если он успешен, продолжай от локального startup payload и требуй reconnect exact front-door-ом: `{reconnect_shell_command}` или `{reconnect_bootstrap_command}`.\n\
4. После startup прочитай runtime artifact `{runtime_state_relative_path}` и obey fail-closed только по полям `{startup_execution_gate_field}`, `{resume_state_field}`, `{resume_contract_field}`, `{resume_obligation_field}`, `{startup_next_action_field}`, `{active_lease_field}`. `{gate_semantics_consistent_field}` должен быть `true`; если resume требует `{required_action_kind}`, follow startup_next_action first. No silent drop.\n\
5. Перед каждым содержательным ответом обновляй `{client_budget_guard_shell_command} --enforce-reply-gate` и работай только по `client_budget_reply_gate.{client_budget_reply_execution_gate_field}`. Начинать user-visible reply можно только с exact non-empty `client_budget_reply_gate.{client_budget_reply_prefix_field}` из `{client_budget_required_reply_prefix_source}`.\n\
6. Fallback для runtime-state inspection: `{startup_state_fallback_shell_command} --repo-root \"{repo_root_display}\" --json`.\n\
7. Любой fail-closed scenario ({fail_closed_conditions}) сообщай как блокер и не угадывай continuity."
    ))
}

fn render_startup_instruction_body(
    workspace_root: &Path,
    helper_repo_root: &Path,
    client_key: &str,
) -> Result<String> {
    let (contract, startup_contract_sha256) =
        startup_contract_for_workspace(workspace_root, helper_repo_root)?;
    let contract_path = startup_contract_artifact_path(workspace_root);
    let agent_contract_path = startup_agent_contract_artifact_path(workspace_root);
    let tool = contract["tool"]
        .as_str()
        .ok_or_else(|| anyhow!("project_chat_startup contract is missing tool"))?;
    let namespace = contract["default_namespace"]
        .as_str()
        .ok_or_else(|| anyhow!("project_chat_startup contract is missing default_namespace"))?;
    let artifact_enforcement = &contract["artifact_enforcement"];
    let _startup_contract_relative_path = artifact_enforcement["workspace_contract_relative_path"]
        .as_str()
        .ok_or_else(|| {
            anyhow!(
                "project_chat_startup contract is missing artifact_enforcement.workspace_contract_relative_path"
            )
        })?;
    let startup_contract_required_before_tool_call =
        artifact_enforcement["workspace_contract_required_before_tool_call"]
            .as_bool()
            .unwrap_or(false);
    let startup_contract_sha256_field = artifact_enforcement["workspace_contract_sha256_field"]
        .as_str()
        .unwrap_or("startup_contract_sha256");
    let startup_contract_missing_or_unreadable_fail_closed =
        artifact_enforcement["missing_or_unreadable_fail_closed"]
            .as_bool()
            .unwrap_or(false);
    let startup_contract_sha256_mismatch_fail_closed =
        artifact_enforcement["sha256_mismatch_fail_closed"]
            .as_bool()
            .unwrap_or(false);
    let tool_runtime_reconcile = &contract["tool_runtime_reconcile"];
    let reconcile_error_class = tool_runtime_reconcile["error_class"]
        .as_str()
        .unwrap_or("tool_execution_failed");
    let reconcile_error_detail_contains = tool_runtime_reconcile["error_detail_contains"]
        .as_str()
        .unwrap_or("no continuity import found for");
    let reconcile_transport_error_detail_contains =
        tool_runtime_reconcile["transport_error_detail_contains"]
            .as_str()
            .unwrap_or("Transport closed");
    let reconcile_transport_error_case_insensitive =
        tool_runtime_reconcile["transport_error_detail_case_insensitive"]
            .as_bool()
            .unwrap_or(false);
    let reconcile_local_cli_command = tool_runtime_reconcile["local_cli"]["command"]
        .as_str()
        .unwrap_or("continuity startup");
    let reconcile_local_cli_shell_command = tool_runtime_reconcile["local_cli"]["shell_command"]
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| format!("cargo run -- {reconcile_local_cli_command}"));
    let reconcile_local_cli_requires_repo_root =
        tool_runtime_reconcile["local_cli"]["requires_repo_root_argument"]
            .as_bool()
            .unwrap_or(false);
    let reconcile_local_cli_requires_namespace =
        tool_runtime_reconcile["local_cli"]["requires_namespace_argument"]
            .as_bool()
            .unwrap_or(false);
    let reconcile_local_cli_json_required = tool_runtime_reconcile["local_cli"]["json_required"]
        .as_bool()
        .unwrap_or(false);
    let reconcile_local_cli_success_classification =
        tool_runtime_reconcile["local_cli_success_classification"]
            .as_str()
            .unwrap_or("stale_embedded_mcp_session");
    let reconcile_local_cli_success_replaces_mcp_failure =
        tool_runtime_reconcile["local_cli_success_replaces_mcp_failure"]
            .as_bool()
            .unwrap_or(false);
    let reconcile_local_cli_success_replaces_transport_failure =
        tool_runtime_reconcile["local_cli_success_replaces_transport_failure"]
            .as_bool()
            .unwrap_or(false);
    let reconcile_must_request_mcp_reconnect =
        tool_runtime_reconcile["must_request_mcp_reconnect_after_local_success"]
            .as_bool()
            .unwrap_or(false);
    let reconcile_must_continue_from_local_payload =
        tool_runtime_reconcile["must_continue_from_local_startup_payload"]
            .as_bool()
            .unwrap_or(false);
    let reconnect_helper_shell_relative_path =
        tool_runtime_reconcile["reconnect_helper"]["shell_helper_relative_path"]
            .as_str()
            .unwrap_or("./scripts/reconnect_local.sh");
    let reconnect_helper_bootstrap_command =
        tool_runtime_reconcile["reconnect_helper"]["bootstrap_command"]
            .as_str()
            .unwrap_or("bootstrap reconnect");
    let reconnect_helper_requires_client_argument =
        tool_runtime_reconcile["reconnect_helper"]["requires_client_argument"]
            .as_bool()
            .unwrap_or(false);
    let reconnect_helper_requires_yes_argument =
        tool_runtime_reconcile["reconnect_helper"]["requires_yes_argument"]
            .as_bool()
            .unwrap_or(false);
    let runtime_state_artifact = &contract["runtime_state_artifact"];
    let runtime_state_relative_path =
        runtime_state_artifact["workspace_runtime_state_relative_path"]
            .as_str()
            .unwrap_or(".amai/continuity/project-chat-startup-state.json");
    let runtime_state_artifact_version =
        runtime_state_artifact["workspace_runtime_state_artifact_version"]
            .as_str()
            .unwrap_or("workspace-startup-runtime-state-v4");
    let runtime_state_written_by_tool = runtime_state_artifact["written_by_tool"]
        .as_str()
        .unwrap_or("amai_continuity_startup");
    let runtime_state_source_summary_field = runtime_state_artifact["source_summary_field"]
        .as_str()
        .unwrap_or("continuity_startup_summary");
    let startup_execution_gate_field = runtime_state_artifact["startup_execution_gate_field"]
        .as_str()
        .unwrap_or("startup_execution_gate");
    let startup_execution_gate_enforcement = &contract["startup_execution_gate_enforcement"];
    let gate_must_follow_field = startup_execution_gate_enforcement["must_follow_field"]
        .as_str()
        .unwrap_or("must_follow_startup_next_action");
    let gate_unrelated_work_allowed_field =
        startup_execution_gate_enforcement["unrelated_work_allowed_field"]
            .as_str()
            .unwrap_or("unrelated_work_allowed");
    let gate_prompt_read_field =
        startup_execution_gate_enforcement["must_read_prompt_text_before_reply_field"]
            .as_str()
            .unwrap_or("must_read_prompt_text_before_reply");
    let gate_required_action_kind_field =
        startup_execution_gate_enforcement["required_action_kind_field"]
            .as_str()
            .unwrap_or("required_action_kind_when_resume_required");
    let gate_no_silent_drop_field = startup_execution_gate_enforcement["no_silent_drop_field"]
        .as_str()
        .unwrap_or("no_silent_drop");
    let gate_semantics_consistent_field = runtime_state_artifact["gate_semantics_consistent_field"]
        .as_str()
        .unwrap_or("gate_semantics_consistent");
    let gate_semantics_consistent_true_required =
        runtime_state_artifact["gate_semantics_consistent_true_required"]
            .as_bool()
            .unwrap_or(false);
    let startup_state_fallback_cli = runtime_state_artifact["inspection_fallback_cli"]["command"]
        .as_str()
        .unwrap_or("continuity startup-state");
    let startup_state_fallback_shell_command =
        runtime_state_artifact["inspection_fallback_cli"]["shell_command"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| format!("cargo run -- {startup_state_fallback_cli}"));
    let fail_closed = contract["fail_closed_conditions"]
        .as_array()
        .ok_or_else(|| anyhow!("project_chat_startup contract is missing fail_closed_conditions"))?
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>()
        .join(", ");
    contract["required_summary_fields"]
        .as_array()
        .ok_or_else(|| {
            anyhow!("project_chat_startup contract is missing required_summary_fields")
        })?;
    contract["restored_obligations"]
        .as_array()
        .ok_or_else(|| anyhow!("project_chat_startup contract is missing restored_obligations"))?;
    let resume_enforcement = &contract["resume_enforcement"];
    let resume_contract_field = resume_enforcement["contract_field"]
        .as_str()
        .ok_or_else(|| {
            anyhow!("project_chat_startup contract is missing resume_enforcement.contract_field")
        })?;
    let resume_state_field = resume_enforcement["resume_state_field"]
        .as_str()
        .ok_or_else(|| {
            anyhow!(
                "project_chat_startup contract is missing resume_enforcement.resume_state_field"
            )
        })?;
    let resume_obligation_field =
        resume_enforcement["obligation_field"]
            .as_str()
            .ok_or_else(|| {
                anyhow!(
                    "project_chat_startup contract is missing resume_enforcement.obligation_field"
                )
            })?;
    let startup_next_action_field = resume_enforcement["startup_next_action_field"]
        .as_str()
        .ok_or_else(|| {
            anyhow!(
                "project_chat_startup contract is missing resume_enforcement.startup_next_action_field"
            )
        })?;
    let active_lease_field = resume_enforcement["active_lease_field"]
        .as_str()
        .ok_or_else(|| {
            anyhow!(
                "project_chat_startup contract is missing resume_enforcement.active_lease_field"
            )
        })?;
    let active_lease_owner_state_field =
        resume_enforcement["active_lease_owner_state_field"]
            .as_str()
            .ok_or_else(|| {
                anyhow!(
                    "project_chat_startup contract is missing resume_enforcement.active_lease_owner_state_field"
                )
            })?;
    let previous_session_owner_value = resume_enforcement["previous_session_owner_value"]
        .as_str()
        .unwrap_or("previous_session_owner");
    let must_resume_before_unrelated =
        resume_enforcement["must_resume_required_return_task_before_unrelated_work"]
            .as_bool()
            .unwrap_or(false);
    let previous_session_owner_must_follow_startup_next_action =
        resume_enforcement["previous_session_owner_must_follow_startup_next_action"]
            .as_bool()
            .unwrap_or(false);
    let required_action_kind = resume_enforcement["required_action_kind_when_resume_required"]
        .as_str()
        .unwrap_or("resume_required_return_task");
    let no_silent_drop = resume_enforcement["no_silent_drop"]
        .as_bool()
        .unwrap_or(false);
    let client_budget_enforcement = &contract["live_client_budget_enforcement"];
    let client_budget_guard_command = client_budget_enforcement["guard_command"]
        .as_str()
        .unwrap_or("observe client-budget-gate");
    let client_budget_guard_shell_command = client_budget_enforcement["guard_shell_command"]
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| format!("cargo run -- {client_budget_guard_command}"));
    let client_budget_guard_summary_field = client_budget_enforcement["guard_summary_field"]
        .as_str()
        .unwrap_or("client_budget_guard");
    let client_budget_reply_execution_gate_field =
        client_budget_enforcement["reply_execution_gate_field"]
            .as_str()
            .unwrap_or("reply_execution_gate");
    let client_budget_reply_execution_gate_version =
        client_budget_enforcement["reply_execution_gate_version"]
            .as_str()
            .unwrap_or("client-reply-budget-gate-v1");
    let client_budget_reply_prefix_field = client_budget_enforcement["reply_prefix_field"]
        .as_str()
        .unwrap_or("reply_prefix");
    let client_budget_reply_prefix_enforcement_flag =
        client_budget_enforcement["reply_prefix_enforcement_flag"]
            .as_str()
            .unwrap_or("--enforce-online-reply-prefix");
    let client_budget_required_reply_prefix_source =
        client_budget_enforcement["required_reply_prefix_source"]
            .as_str()
            .unwrap_or("personal_agent_online_limit_contour");
    let client_budget_required_reply_prefix_non_empty =
        client_budget_enforcement["required_reply_prefix_non_empty"]
            .as_bool()
            .unwrap_or(true);
    let client_budget_reply_prefix_preflight_blocks_substantive_reply =
        client_budget_enforcement["reply_prefix_preflight_blocks_substantive_reply"]
            .as_bool()
            .unwrap_or(true);
    let client_budget_output_prefix_enforcement_mode =
        client_budget_enforcement["output_prefix_enforcement_mode"]
            .as_str()
            .unwrap_or("instruction_preflight_fail_closed");
    let client_budget_output_prefix_host_enforced =
        client_budget_enforcement["output_prefix_host_enforced"]
            .as_bool()
            .unwrap_or(false);
    let client_budget_reply_budget_mode_field =
        client_budget_enforcement["reply_budget_mode_field"]
            .as_str()
            .unwrap_or("reply_budget_mode");
    let client_budget_reply_budget_contract_field =
        client_budget_enforcement["reply_budget_contract_field"]
            .as_str()
            .unwrap_or("reply_budget_contract");
    let client_budget_compact_reply_mode_value =
        client_budget_enforcement["compact_reply_mode_value"]
            .as_str()
            .unwrap_or(working_state::CLIENT_REPLY_BUDGET_MODE_COMPACT_HIGH_SIGNAL);
    let client_budget_compact_reply_contract_version =
        client_budget_enforcement["compact_reply_contract_version"]
            .as_str()
            .unwrap_or(working_state::CLIENT_REPLY_BUDGET_CONTRACT_VERSION);
    let client_budget_guard_enforcement_flag = client_budget_enforcement["guard_enforcement_flag"]
        .as_str()
        .unwrap_or("--enforce-reply-gate");
    let client_budget_guard_enforcement_exit_on_blocking =
        client_budget_enforcement["guard_enforcement_exit_on_blocking"]
            .as_bool()
            .unwrap_or(true);
    let client_budget_compact_diagnostics_command =
        client_budget_enforcement["compact_diagnostics_command"]
            .as_str()
            .unwrap_or("observe client-budget-root-cause");
    let client_budget_compact_diagnostics_shell_command =
        client_budget_enforcement["compact_diagnostics_shell_command"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| format!("cargo run -- {client_budget_compact_diagnostics_command}"));
    let client_budget_prefer_compact_diagnostics =
        client_budget_enforcement["must_prefer_compact_diagnostics_over_full_snapshot"]
            .as_bool()
            .unwrap_or(false);
    let client_budget_must_check_before_each_reply =
        client_budget_enforcement["must_check_before_each_substantive_reply"]
            .as_bool()
            .unwrap_or(false);
    let client_budget_continuity_write_exempt_from_reply_guard =
        client_budget_enforcement["continuity_write_exempt_from_reply_guard"]
            .as_bool()
            .unwrap_or(false);
    let client_budget_continuity_write_required_before_rotate =
        client_budget_enforcement["continuity_write_required_before_rotate"]
            .as_bool()
            .unwrap_or(false);
    let client_budget_continuity_write_operations =
        client_budget_enforcement["continuity_write_operations"]
            .as_array()
            .map(|values| {
                values
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "continuity handoff".to_string());
    let client_budget_max_guard_age_seconds = client_budget_enforcement["max_guard_age_seconds"]
        .as_u64()
        .unwrap_or(10);
    let client_budget_stale_guard_requires_refresh =
        client_budget_enforcement["stale_guard_requires_refresh"]
            .as_bool()
            .unwrap_or(false);
    let client_budget_rotate_now_field = client_budget_enforcement["rotate_now_field"]
        .as_str()
        .unwrap_or("should_rotate_chat_now");
    let client_budget_status_label_field = client_budget_enforcement["status_label_field"]
        .as_str()
        .unwrap_or("status_label");
    let client_budget_rotate_status_labels = client_budget_enforcement["rotate_status_labels"]
        .as_array()
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(|value| client_turn_pressure_display_status_label(value, true))
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>()
                .join(", ")
        })
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| CLIENT_TURN_PRESSURE_ROTATE_STATUS_LABELS.join(", "));
    let client_budget_save_handoff_before_rotate =
        client_budget_enforcement["save_handoff_before_rotate"]
            .as_bool()
            .unwrap_or(false);
    let client_budget_fresh_chat_requires_startup =
        client_budget_enforcement["fresh_chat_requires_continuity_startup"]
            .as_bool()
            .unwrap_or(false);
    let client_budget_full_scale_truth_required =
        client_budget_enforcement["full_scale_client_truth_required"]
            .as_bool()
            .unwrap_or(false);
    let client_budget_reply_blocking_removed = client_budget_enforcement["reply_blocking_removed"]
        .as_bool()
        .unwrap_or(false);
    let client_budget_tool_turn_blocking_removed =
        client_budget_enforcement["tool_turn_blocking_removed"]
            .as_bool()
            .unwrap_or(false);
    let _client_budget_blocking_reply_contract_field =
        client_budget_enforcement["blocking_reply_contract_field"]
            .as_str()
            .unwrap_or("blocking_reply_contract");
    let _client_budget_blocking_reply_contract_version =
        client_budget_enforcement["blocking_reply_contract_version"]
            .as_str()
            .unwrap_or(working_state::CLIENT_BUDGET_BLOCKING_REPLY_CONTRACT_VERSION);
    let _client_budget_blocking_reply_response_kind =
        client_budget_enforcement["blocking_reply_response_kind"]
            .as_str()
            .unwrap_or(working_state::CLIENT_BUDGET_BLOCKING_REPLY_RESPONSE_KIND);
    let _client_budget_blocking_reply_max_sentences =
        client_budget_enforcement["blocking_reply_max_sentences"]
            .as_u64()
            .unwrap_or(working_state::CLIENT_BUDGET_BLOCKING_REPLY_MAX_SENTENCES);
    let client_budget_blocking_reply_must_avoid_substantive_work =
        client_budget_enforcement["blocking_reply_must_avoid_substantive_work"]
            .as_bool()
            .unwrap_or(true);
    let client_budget_blocking_reply_must_use_action_bundle_operator_flow =
        client_budget_enforcement["blocking_reply_must_use_action_bundle_operator_flow"]
            .as_bool()
            .unwrap_or(true);
    let _client_budget_blocking_reply_template =
        client_budget_enforcement["blocking_reply_template"]
            .as_str()
            .unwrap_or(working_state::CLIENT_BUDGET_BLOCKING_REPLY_TEMPLATE);
    let client_budget_target_control = &client_budget_enforcement["target_control"];
    let client_budget_target_command_pattern =
        client_budget_target_control["exact_chat_command_pattern"]
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(continuity::client_budget_target_chat_command_pattern);
    let _client_budget_target_allowed_percents =
        client_budget_target_control["allowed_target_percents"]
            .as_array()
            .map(|values| {
                values
                    .iter()
                    .filter_map(Value::as_u64)
                    .map(|value| value.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| {
                continuity::allowed_client_budget_target_values()
                    .into_iter()
                    .map(|value| value.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            });
    let client_budget_target_cli_command = client_budget_target_control["cli_command"]
        .as_str()
        .unwrap_or("continuity client-budget-target");
    let client_budget_target_shell_command = client_budget_target_control["shell_command"]
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| format!("cargo run -- {client_budget_target_cli_command}"));
    let client_budget_target_percent_argument = client_budget_target_control["percent_argument"]
        .as_str()
        .unwrap_or("--percent");
    let client_budget_target_namespace_argument =
        client_budget_target_control["namespace_argument"]
            .as_str()
            .unwrap_or("--namespace");
    let client_budget_target_repo_root_argument_required =
        client_budget_target_control["repo_root_argument_required"]
            .as_bool()
            .unwrap_or(true);
    let client_budget_target_switch_immediately =
        client_budget_target_control["switch_immediately_on_exact_chat_command"]
            .as_bool()
            .unwrap_or(true);
    let client_budget_target_reply_with_confirmation =
        client_budget_target_control["reply_with_confirmation_after_switch"]
            .as_bool()
            .unwrap_or(true);
    let client_budget_target_example_command = continuity::client_budget_target_chat_command(50);
    let client_budget_compact_chat_control = &client_budget_enforcement["compact_chat_control"];
    let client_budget_compact_chat_exact_command =
        client_budget_compact_chat_control["exact_chat_command"]
            .as_str()
            .unwrap_or(continuity::CLIENT_BUDGET_COMPACT_CHAT_COMMAND);
    let client_budget_compact_chat_cli_command = client_budget_compact_chat_control["cli_command"]
        .as_str()
        .unwrap_or("continuity compact-chat");
    let client_budget_compact_chat_shell_command =
        client_budget_compact_chat_control["shell_command"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| format!("cargo run -- {client_budget_compact_chat_cli_command}"));
    let client_budget_compact_chat_namespace_argument =
        client_budget_compact_chat_control["namespace_argument"]
            .as_str()
            .unwrap_or("--namespace");
    let client_budget_compact_chat_repo_root_argument_required = client_budget_compact_chat_control
        ["repo_root_argument_required"]
        .as_bool()
        .unwrap_or(true);
    let client_budget_compact_chat_switch_immediately =
        client_budget_compact_chat_control["switch_immediately_on_exact_chat_command"]
            .as_bool()
            .unwrap_or(true);
    let client_budget_compact_chat_reply_with_confirmation =
        client_budget_compact_chat_control["reply_with_confirmation_after_prepare"]
            .as_bool()
            .unwrap_or(true);
    let client_budget_compact_chat_prompt_text_required =
        client_budget_compact_chat_control["prompt_text_required_for_rebase"]
            .as_bool()
            .unwrap_or(true);
    let client_budget_compact_chat_required_host_action =
        client_budget_compact_chat_control["required_host_action"]
            .as_str()
            .unwrap_or(
                "open_clean_chat_surface_and_inject_prompt_text_if_launch_bridge_unavailable",
            );
    let client_budget_max_guard_age_seconds_text = client_budget_max_guard_age_seconds.to_string();
    let client_budget_stale_guard_requires_refresh_text =
        if client_budget_stale_guard_requires_refresh {
            "true"
        } else {
            "false"
        };
    let client_budget_save_handoff_before_rotate_text = if client_budget_save_handoff_before_rotate
    {
        "true"
    } else {
        "false"
    };
    let client_budget_fresh_chat_requires_startup_text =
        if client_budget_fresh_chat_requires_startup {
            "true"
        } else {
            "false"
        };
    let _client_budget_blocking_reply_must_avoid_substantive_work_text =
        if client_budget_blocking_reply_must_avoid_substantive_work {
            "true"
        } else {
            "false"
        };
    let _client_budget_blocking_reply_must_use_action_bundle_operator_flow_text =
        if client_budget_blocking_reply_must_use_action_bundle_operator_flow {
            "true"
        } else {
            "false"
        };
    let client_budget_target_repo_root_argument_required_text =
        if client_budget_target_repo_root_argument_required {
            "true"
        } else {
            "false"
        };
    let client_budget_target_switch_immediately_text = if client_budget_target_switch_immediately {
        "true"
    } else {
        "false"
    };
    let client_budget_target_reply_with_confirmation_text =
        if client_budget_target_reply_with_confirmation {
            "true"
        } else {
            "false"
        };
    let client_budget_full_scale_truth_required_text = if client_budget_full_scale_truth_required {
        "true"
    } else {
        "false"
    };
    let client_budget_reply_blocking_removed_text = if client_budget_reply_blocking_removed {
        "true"
    } else {
        "false"
    };
    let client_budget_tool_turn_blocking_removed_text = if client_budget_tool_turn_blocking_removed
    {
        "true"
    } else {
        "false"
    };
    let startup_contract_required_before_tool_call_text =
        if startup_contract_required_before_tool_call {
            "true"
        } else {
            "false"
        };
    let startup_contract_missing_or_unreadable_fail_closed_text =
        if startup_contract_missing_or_unreadable_fail_closed {
            "true"
        } else {
            "false"
        };
    let startup_contract_sha256_mismatch_fail_closed_text =
        if startup_contract_sha256_mismatch_fail_closed {
            "true"
        } else {
            "false"
        };
    let gate_semantics_consistent_true_required_text = if gate_semantics_consistent_true_required {
        "true"
    } else {
        "false"
    };
    let must_resume_before_unrelated_text = if must_resume_before_unrelated {
        "true"
    } else {
        "false"
    };
    let previous_session_owner_must_follow_startup_next_action_text =
        if previous_session_owner_must_follow_startup_next_action {
            "true"
        } else {
            "false"
        };
    let no_silent_drop_text = if no_silent_drop { "true" } else { "false" };
    let client_budget_must_check_before_each_reply_text =
        if client_budget_must_check_before_each_reply {
            "true"
        } else {
            "false"
        };
    let client_budget_guard_enforcement_exit_on_blocking_text =
        if client_budget_guard_enforcement_exit_on_blocking {
            "true"
        } else {
            "false"
        };
    let client_budget_continuity_write_exempt_from_reply_guard_text =
        if client_budget_continuity_write_exempt_from_reply_guard {
            "true"
        } else {
            "false"
        };
    let client_budget_continuity_write_required_before_rotate_text =
        if client_budget_continuity_write_required_before_rotate {
            "true"
        } else {
            "false"
        };
    let client_budget_prefer_compact_diagnostics_text = if client_budget_prefer_compact_diagnostics
    {
        "true"
    } else {
        "false"
    };
    let repo_root_display = workspace_root.display().to_string();
    let contract_path_display = contract_path.display().to_string();
    let agent_contract_path_display = agent_contract_path.display().to_string();
    let _startup_agent_contract_relative_path =
        ".amai/onboarding/project-chat-startup-agent-contract.json";
    let reconnect_shell_command = if helper_repo_root != workspace_root {
        let helper = helper_repo_root.display();
        if reconnect_helper_requires_client_argument {
            format!(
                "{reconnect_helper_shell_relative_path} --client {client_key} --cwd {} --workspace-root {} --yes",
                helper,
                workspace_root.display()
            )
        } else {
            format!(
                "{reconnect_helper_shell_relative_path} --cwd {} --workspace-root {} --yes",
                helper,
                workspace_root.display()
            )
        }
    } else if reconnect_helper_requires_client_argument {
        format!("{reconnect_helper_shell_relative_path} --client {client_key}")
    } else {
        reconnect_helper_shell_relative_path.to_string()
    };
    let reconnect_bootstrap_command = if helper_repo_root != workspace_root {
        let helper_exec = helper_repo_root.join("scripts/amai_exec.sh");
        if reconnect_helper_requires_client_argument {
            format!(
                "{} {} --client {client_key} --cwd {} --workspace-root {} --yes",
                helper_exec.display(),
                reconnect_helper_bootstrap_command,
                helper_repo_root.display(),
                workspace_root.display()
            )
        } else {
            format!(
                "{} {} --cwd {} --workspace-root {} --yes",
                helper_exec.display(),
                reconnect_helper_bootstrap_command,
                helper_repo_root.display(),
                workspace_root.display()
            )
        }
    } else if reconnect_helper_requires_client_argument {
        if reconnect_helper_requires_yes_argument {
            format!(
                "./scripts/amai_exec.sh {reconnect_helper_bootstrap_command} --client {client_key} --yes"
            )
        } else {
            format!(
                "./scripts/amai_exec.sh {reconnect_helper_bootstrap_command} --client {client_key}"
            )
        }
    } else if reconnect_helper_requires_yes_argument {
        format!("./scripts/amai_exec.sh {reconnect_helper_bootstrap_command} --yes")
    } else {
        format!("./scripts/amai_exec.sh {reconnect_helper_bootstrap_command}")
    };
    let reconcile_transport_error_case_insensitive_text =
        if reconcile_transport_error_case_insensitive {
            "true"
        } else {
            "false"
        };
    let reconcile_local_cli_success_replaces_transport_failure_text =
        if reconcile_local_cli_success_replaces_transport_failure {
            "true"
        } else {
            "false"
        };
    let client_budget_required_reply_prefix_non_empty_text =
        if client_budget_required_reply_prefix_non_empty {
            "true"
        } else {
            "false"
        };
    let client_budget_reply_prefix_preflight_blocks_substantive_reply_text =
        if client_budget_reply_prefix_preflight_blocks_substantive_reply {
            "true"
        } else {
            "false"
        };
    let client_budget_output_prefix_host_enforced_text =
        if client_budget_output_prefix_host_enforced {
            "true"
        } else {
            "false"
        };

    let instructions = [
        "Перед первым содержательным ответом в новом или resumed чате и дальше перед каждым следующим содержательным ответом:".to_string(),
        format!(
            "1. Workspace = `{repo_root_display}`. Прочитай compact agent contract `{agent_contract_path_display}` и machine-readable startup contract `{contract_path_display}`; startup contract остаётся pinned source-of-truth. До MCP tool call проверь `{startup_contract_sha256_field} = \"{startup_contract_sha256}\"`, `workspace_contract_required_before_tool_call = {startup_contract_required_before_tool_call_text}`, `missing_or_unreadable_fail_closed = {startup_contract_missing_or_unreadable_fail_closed_text}`, `sha256_mismatch_fail_closed = {startup_contract_sha256_mismatch_fail_closed_text}`."
        ),
        format!(
            "2. Затем вызови MCP tool `{tool}` с `repo_root = \"{repo_root_display}\"` и `namespace = \"{namespace}\"`; `project` передавай только при exact binding по repo_root. До `continuity_startup_summary` не переходи к `amai_context_pack` и новым действиям. Если tool вернул `{reconcile_error_class}` и detail содержит `{reconcile_error_detail_contains}`, немедленно сделай reconcile через `{reconcile_local_cli_shell_command} --repo-root \"{repo_root_display}\" --namespace \"{namespace}\" --json` (`requires_repo_root_argument = {reconcile_local_cli_requires_repo_root}`, `requires_namespace_argument = {reconcile_local_cli_requires_namespace}`, `json_required = {reconcile_local_cli_json_required}`). Если embedded MCP tool call упал на transport error с detail `{reconcile_transport_error_detail_contains}` (`transport_error_detail_case_insensitive = {reconcile_transport_error_case_insensitive_text}`), делай тот же local CLI reconcile. Если local CLI startup succeeds, классифицируй это как `{reconcile_local_cli_success_classification}` (`local_cli_success_replaces_mcp_failure = {reconcile_local_cli_success_replaces_mcp_failure}`, `local_cli_success_replaces_transport_failure = {reconcile_local_cli_success_replaces_transport_failure_text}`, `must_request_mcp_reconnect_after_local_success = {reconcile_must_request_mcp_reconnect}`, `must_continue_from_local_startup_payload = {reconcile_must_continue_from_local_payload}`), продолжай от локального startup payload и требуй reconnect exact front-door-ом: `{reconnect_shell_command}` или `{reconnect_bootstrap_command}`. Только если и MCP, и local CLI fallback провалились, объявляй continuity реально unavailable."
        ),
        format!(
            "3. После startup прочитай runtime artifact `{runtime_state_relative_path}`: `workspace_runtime_state_artifact_version` должен быть `{runtime_state_artifact_version}`, его пишет `{runtime_state_written_by_tool}`, он обязан нести `{runtime_state_source_summary_field}`. Fallback: `{startup_state_fallback_shell_command} --repo-root \"{repo_root_display}\" --json`."
        ),
        format!(
            "4. В runtime artifact смотри только `{startup_execution_gate_field}`, `{resume_state_field}`, `{resume_contract_field}`, `{resume_obligation_field}`, `{startup_next_action_field}`, `{active_lease_field}`. Restore бери из `required_summary_fields`, obligations из `restored_obligations`. Fail-closed, если `{gate_semantics_consistent_field} != true` (`gate_semantics_consistent_true_required = {gate_semantics_consistent_true_required_text}`), `{startup_execution_gate_field}.{gate_must_follow_field} != true`, `{startup_execution_gate_field}.{gate_unrelated_work_allowed_field} != false`, `{startup_execution_gate_field}.{gate_prompt_read_field} != true` или `{startup_execution_gate_field}.{gate_no_silent_drop_field} != true`."
        ),
        format!(
            "5. Resume law: если `{startup_execution_gate_field}.{gate_required_action_kind_field} == \"{required_action_kind}\"`, `{startup_next_action_field}.action_kind == \"{required_action_kind}\"` (`must_resume_required_return_task_before_unrelated_work = {must_resume_before_unrelated_text}`) или `{active_lease_field}.{active_lease_owner_state_field} == \"{previous_session_owner_value}\"` (`previous_session_owner_must_follow_startup_next_action = {previous_session_owner_must_follow_startup_next_action_text}`), follow startup_next_action first. `no_silent_drop = {no_silent_drop_text}`. Для resume смотри `execctl_active_lease_summary`, `required_return_task`, `required_task_set`, `required_task_set_summary`, `project_task_tree`, `project_task_tree_summary`, `project_task_ledger`, `project_task_ledger_summary`."
        ),
        format!(
            "6. Перед каждым содержательным ответом обновляй guard `{client_budget_guard_shell_command}` и работай только по `{client_budget_guard_summary_field}.{client_budget_reply_execution_gate_field}`. `must_check_before_each_substantive_reply = {client_budget_must_check_before_each_reply_text}`; stale старше `{client_budget_max_guard_age_seconds_text}` секунд запрещён (`stale_guard_requires_refresh = {client_budget_stale_guard_requires_refresh_text}`). Hard gate automation: `{client_budget_guard_enforcement_flag}` (`guard_enforcement_exit_on_blocking = {client_budget_guard_enforcement_exit_on_blocking_text}`). Prefix preflight: `{client_budget_reply_prefix_enforcement_flag}` (`required_reply_prefix_source = {client_budget_required_reply_prefix_source}`, `required_reply_prefix_non_empty = {client_budget_required_reply_prefix_non_empty_text}`, `reply_prefix_preflight_blocks_substantive_reply = {client_budget_reply_prefix_preflight_blocks_substantive_reply_text}`, `output_prefix_enforcement_mode = {client_budget_output_prefix_enforcement_mode}`, `output_prefix_host_enforced = {client_budget_output_prefix_host_enforced_text}`). Continuity write-side maintenance в Amai ({client_budget_continuity_write_operations}) не блокируется reply guard (`continuity_write_exempt_from_reply_guard = {client_budget_continuity_write_exempt_from_reply_guard_text}`) и при rotate/advisory pressure остаётся обязательным перед уходом (`continuity_write_required_before_rotate = {client_budget_continuity_write_required_before_rotate_text}`). Для KPI/guard/exact-pair root-cause сначала используй `{client_budget_compact_diagnostics_shell_command}`; `must_prefer_compact_diagnostics_over_full_snapshot = {client_budget_prefer_compact_diagnostics_text}`."
        ),
        format!(
            "7. Gate version pinned: `{client_budget_reply_execution_gate_version}`. Начинать user-visible reply можно только если `{client_budget_reply_execution_gate_field}.{client_budget_reply_prefix_field}` не пустой и источник равен `{client_budget_required_reply_prefix_source}`; иначе substantive reply запрещён и сначала нужен новый guard-check через `{client_budget_reply_prefix_enforcement_flag}`. Если prefix готов, начинай reply с этой exact строки. Если `{client_budget_reply_budget_mode_field} == \"{client_budget_compact_reply_mode_value}\"`, substantive reply разрешён только по `{client_budget_reply_budget_contract_field}` с `contract_version = \"{client_budget_compact_reply_contract_version}\"`: direct answer first, no unrequested recap, no repeated known context, keep only changed facts, prefer patch/result over narration when coding, preserve truthfulness/technical accuracy, disclose unknowns instead of guessing. Exact operator-switch для target режима: matching `{client_budget_target_command_pattern}` -> `{client_budget_target_shell_command} --repo-root \"{repo_root_display}\" {client_budget_target_namespace_argument} \"{namespace}\" {client_budget_target_percent_argument} N` (`repo_root_argument_required = {client_budget_target_repo_root_argument_required_text}`, `switch_immediately_on_exact_chat_command = {client_budget_target_switch_immediately_text}`, `reply_with_confirmation_after_switch = {client_budget_target_reply_with_confirmation_text}`). Пример exact chat-команды: `{client_budget_target_example_command}`. Exact operator-switch для huge-chat rebase: точную команду `{client_budget_compact_chat_exact_command}` обработай через `{client_budget_compact_chat_shell_command} --repo-root \"{repo_root_display}\" {client_budget_compact_chat_namespace_argument} \"{namespace}\" --json` (`repo_root_argument_required = {client_budget_compact_chat_repo_root_argument_required}`, `switch_immediately_on_exact_chat_command = {client_budget_compact_chat_switch_immediately}`, `reply_with_confirmation_after_prepare = {client_budget_compact_chat_reply_with_confirmation}`, `prompt_text_required_for_rebase = {client_budget_compact_chat_prompt_text_required}`), верни `prompt_text` и `operator_notice`, и требуй host action `{client_budget_compact_chat_required_host_action}`."
        ),
        format!(
            "8. Client-budget blocked reply mechanism removed: `reply_blocking_removed = {client_budget_reply_blocking_removed_text}`. Tool-turn blocked mechanism removed too: `tool_turn_blocking_removed = {client_budget_tool_turn_blocking_removed_text}`. Если `{client_budget_reply_execution_gate_field}.must_rotate_before_reply = true`, `{client_budget_reply_execution_gate_field}.must_wait_for_budget_recovery_before_reply = true`, `{client_budget_rotate_now_field} = true`, `{client_budget_status_label_field}` равен одному из current normalized same-thread advisory labels [{client_budget_rotate_status_labels}], `same_meter_pure_burn_turn_active = true`, `must_avoid_new_tool_turn_without_specific_delta_goal = true` или `max_tool_roundtrips_soft = 0`, считай это только advisory/compact pressure signal. Этот список в startup instructions является non-binding human-readable snapshot канонического shared advisory source, а не отдельным policy-list. User-visible blocked wait template использовать запрещено; `amai_context_pack`, continuity write и другие Amai tools не блокируй только из-за этих полей. `save_handoff_before_rotate = {client_budget_save_handoff_before_rotate_text}` и `fresh_chat_requires_continuity_startup = {client_budget_fresh_chat_requires_startup_text}` остаются operator guidance."
        ),
        format!(
            "9. Не подменяй полную клиентскую шкалу внутренним Amai-slice: `full_scale_client_truth_required = {client_budget_full_scale_truth_required_text}`. Любой fail-closed scenario ({fail_closed}) сообщай как блокер и не угадывай continuity."
        ),
    ]
    .join("\n");

    Ok(instructions)
}

fn render_startup_contract_artifact(
    workspace_root: &Path,
    helper_repo_root: &Path,
) -> Result<(String, String)> {
    let (contract, startup_contract_sha256) =
        startup_contract_for_workspace(workspace_root, helper_repo_root)?;
    let tool = contract["tool"]
        .as_str()
        .ok_or_else(|| anyhow!("project_chat_startup contract is missing tool"))?;
    let namespace = contract["default_namespace"]
        .as_str()
        .ok_or_else(|| anyhow!("project_chat_startup contract is missing default_namespace"))?;
    let payload = json!({
        "artifact_version": "workspace-startup-contract-v1",
        "contract_kind": "project_chat_startup",
        "repo_root": workspace_root.display().to_string(),
        "default_namespace": namespace,
        "startup_contract_sha256": startup_contract_sha256,
        "startup_contract_sha256_scope": "startup_contract object only",
        "tool": tool,
        "recommended_startup_call": {
            "tool": tool,
            "arguments": {
                "repo_root": workspace_root.display().to_string(),
                "namespace": namespace
            },
            "project_argument_rule": "pass project when already known, otherwise require exact binding by repo_root"
        },
        "startup_contract": contract
    });
    let content =
        serde_json::to_string(&payload).context("failed to serialize startup contract artifact")?;
    Ok((content, startup_contract_sha256))
}

fn render_startup_agent_contract_artifact(
    workspace_root: &Path,
    helper_repo_root: &Path,
) -> Result<String> {
    let (contract, startup_contract_sha256) =
        startup_contract_for_workspace(workspace_root, helper_repo_root)?;
    let tool = contract["tool"]
        .as_str()
        .ok_or_else(|| anyhow!("project_chat_startup contract is missing tool"))?;
    let namespace = contract["default_namespace"]
        .as_str()
        .ok_or_else(|| anyhow!("project_chat_startup contract is missing default_namespace"))?;
    let payload = json!({
        "artifact_version": "workspace-startup-agent-contract-v1",
        "contract_kind": "project_chat_startup_agent_read",
        "repo_root": workspace_root.display().to_string(),
        "default_namespace": namespace,
        "tool": tool,
        "full_startup_contract_relative_path": ".amai/onboarding/project-chat-startup-contract.json",
        "full_startup_contract_sha256": startup_contract_sha256,
        "recommended_startup_call": {
            "tool": tool,
            "arguments": {
                "repo_root": workspace_root.display().to_string(),
                "namespace": namespace
            }
        },
        "runtime_state_artifact": {
            "workspace_runtime_state_relative_path": contract["runtime_state_artifact"]["workspace_runtime_state_relative_path"].clone(),
            "workspace_runtime_state_artifact_version": contract["runtime_state_artifact"]["workspace_runtime_state_artifact_version"].clone(),
            "source_summary_field": contract["runtime_state_artifact"]["source_summary_field"].clone(),
            "startup_execution_gate_field": contract["runtime_state_artifact"]["startup_execution_gate_field"].clone(),
            "gate_semantics_consistent_field": contract["runtime_state_artifact"]["gate_semantics_consistent_field"].clone(),
            "written_by_tool": contract["runtime_state_artifact"]["written_by_tool"].clone(),
            "inspection_fallback_cli_command": contract["runtime_state_artifact"]["inspection_fallback_cli"]["command"].clone(),
            "inspection_fallback_cli_shell_command": contract["runtime_state_artifact"]["inspection_fallback_cli"]["shell_command"].clone()
        },
        "required_summary_fields": contract["required_summary_fields"].clone(),
        "restored_obligations": contract["restored_obligations"].clone(),
        "compact_runtime_pointers": {
            "resume_state_field": contract["resume_enforcement"]["resume_state_field"].clone(),
            "resume_contract_field": contract["resume_enforcement"]["contract_field"].clone(),
            "resume_obligation_field": contract["resume_enforcement"]["obligation_field"].clone(),
            "startup_next_action_field": contract["resume_enforcement"]["startup_next_action_field"].clone(),
            "active_lease_field": contract["resume_enforcement"]["active_lease_field"].clone(),
            "previous_session_owner_value": contract["resume_enforcement"]["previous_session_owner_value"].clone(),
            "required_action_kind_when_resume_required": contract["resume_enforcement"]["required_action_kind_when_resume_required"].clone(),
            "reconcile_local_cli_shell_command": contract["tool_runtime_reconcile"]["local_cli"]["shell_command"].clone(),
            "guard_command": contract["live_client_budget_enforcement"]["guard_command"].clone(),
            "guard_shell_command": contract["live_client_budget_enforcement"]["guard_shell_command"].clone(),
            "compact_diagnostics_command": contract["live_client_budget_enforcement"]["compact_diagnostics_command"].clone(),
            "compact_diagnostics_shell_command": contract["live_client_budget_enforcement"]["compact_diagnostics_shell_command"].clone(),
            "startup_state_fallback_shell_command": contract["runtime_state_artifact"]["inspection_fallback_cli"]["shell_command"].clone(),
            "guard_enforcement_flag": contract["live_client_budget_enforcement"]["guard_enforcement_flag"].clone(),
            "reply_execution_gate_field": contract["live_client_budget_enforcement"]["reply_execution_gate_field"].clone(),
            "reply_prefix_field": contract["live_client_budget_enforcement"]["reply_prefix_field"].clone(),
            "reply_prefix_enforcement_flag": contract["live_client_budget_enforcement"]["reply_prefix_enforcement_flag"].clone(),
            "required_reply_prefix_source": contract["live_client_budget_enforcement"]["required_reply_prefix_source"].clone(),
            "compact_reply_mode_value": contract["live_client_budget_enforcement"]["compact_reply_mode_value"].clone(),
            "blocking_reply_contract_field": contract["live_client_budget_enforcement"]["blocking_reply_contract_field"].clone(),
            "blocking_reply_template": contract["live_client_budget_enforcement"]["blocking_reply_template"].clone(),
            "client_budget_target_exact_chat_command_pattern": contract["live_client_budget_enforcement"]["target_control"]["exact_chat_command_pattern"].clone(),
            "client_budget_target_allowed_percents": contract["live_client_budget_enforcement"]["target_control"]["allowed_target_percents"].clone(),
            "client_budget_target_cli_command": contract["live_client_budget_enforcement"]["target_control"]["cli_command"].clone(),
            "client_budget_target_shell_command": contract["live_client_budget_enforcement"]["target_control"]["shell_command"].clone(),
            "client_budget_compact_chat_exact_chat_command": contract["live_client_budget_enforcement"]["compact_chat_control"]["exact_chat_command"].clone(),
            "client_budget_compact_chat_cli_command": contract["live_client_budget_enforcement"]["compact_chat_control"]["cli_command"].clone(),
            "client_budget_compact_chat_shell_command": contract["live_client_budget_enforcement"]["compact_chat_control"]["shell_command"].clone(),
            "client_budget_compact_chat_required_host_action": contract["live_client_budget_enforcement"]["compact_chat_control"]["required_host_action"].clone()
        },
        "fail_closed_conditions": contract["fail_closed_conditions"].clone()
    });
    serde_json::to_string(&payload).context("failed to serialize startup agent contract artifact")
}

#[derive(Debug, Deserialize)]
struct OpenClawAgentListEntry {
    id: String,
    #[serde(default)]
    workspace: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenClawAgentAddSummary {
    #[serde(rename = "agentId")]
    agent_id: String,
    #[serde(rename = "workspace")]
    _workspace: String,
}

fn openclaw_base_command(config_path: &Path) -> std::process::Command {
    let mut command = std::process::Command::new("openclaw");
    command.env("OPENCLAW_CONFIG_PATH", config_path);
    command.env("OPENCLAW_HIDE_BANNER", "1");
    command
}

fn openclaw_agent_id(repo_root: &Path) -> String {
    let basename = repo_root
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("repo")
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    let digest = hex::encode(Sha256::digest(repo_root.display().to_string().as_bytes()));
    format!("amai-{}-{}", basename, &digest[..8])
}

fn hermes_profile_id(repo_root: &Path) -> String {
    let basename = repo_root
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("repo")
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    let digest = hex::encode(Sha256::digest(repo_root.display().to_string().as_bytes()));
    format!("amai-{}-{}", basename, &digest[..8])
}

fn hermes_runtime_artifact_path(repo_root: &Path) -> PathBuf {
    repo_root
        .join(".amai")
        .join("onboarding")
        .join("hermes-profile-runtime.json")
}

fn hermes_default_home() -> Result<PathBuf> {
    Ok(home_dir()
        .ok_or_else(|| anyhow!("failed to resolve user home directory for Hermes runtime"))?
        .join(".hermes"))
}

fn hermes_profiles_root() -> Result<PathBuf> {
    Ok(hermes_default_home()?.join("profiles"))
}

fn hermes_profile_dir(profile_name: &str) -> Result<PathBuf> {
    Ok(hermes_profiles_root()?.join(profile_name))
}

fn hermes_active_profile_path() -> Result<PathBuf> {
    Ok(hermes_default_home()?.join("active_profile"))
}

fn read_hermes_active_profile() -> Result<String> {
    let path = hermes_active_profile_path()?;
    match fs::read_to_string(&path) {
        Ok(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                Ok("default".to_string())
            } else {
                Ok(trimmed.to_string())
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok("default".to_string()),
        Err(error) => Err(error).with_context(|| format!("failed to read {}", path.display())),
    }
}

fn write_hermes_active_profile(profile_name: &str) -> Result<()> {
    let path = hermes_active_profile_path()?;
    if profile_name == "default" {
        if path.exists() {
            fs::remove_file(&path)
                .with_context(|| format!("failed to remove {}", path.display()))?;
        }
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    fs::write(&path, format!("{profile_name}\n"))
        .with_context(|| format!("failed to write {}", path.display()))
}

fn ensure_hermes_profile_dirs(profile_dir: &Path) -> Result<()> {
    for dir in [
        "memories",
        "sessions",
        "skills",
        "skins",
        "logs",
        "plans",
        "workspace",
        "cron",
        "home",
    ] {
        fs::create_dir_all(profile_dir.join(dir)).with_context(|| {
            format!(
                "failed to create Hermes profile directory {}",
                profile_dir.join(dir).display()
            )
        })?;
    }
    Ok(())
}

fn copy_optional_file(src: &Path, dst: &Path) -> Result<()> {
    if !src.is_file() {
        return Ok(());
    }
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    fs::copy(src, dst).with_context(|| {
        format!(
            "failed to copy optional file {} -> {}",
            src.display(),
            dst.display()
        )
    })?;
    Ok(())
}

fn copy_optional_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    if !src.is_dir() {
        return Ok(());
    }
    fs::create_dir_all(dst).with_context(|| format!("failed to create {}", dst.display()))?;
    for entry in fs::read_dir(src).with_context(|| format!("failed to read {}", src.display()))? {
        let entry = entry.with_context(|| format!("failed to inspect {}", src.display()))?;
        let path = entry.path();
        let dest = dst.join(entry.file_name());
        let file_type = entry
            .file_type()
            .with_context(|| format!("failed to inspect {}", path.display()))?;
        if file_type.is_dir() {
            copy_optional_dir_recursive(&path, &dest)?;
        } else if file_type.is_file() {
            copy_optional_file(&path, &dest)?;
        }
    }
    Ok(())
}

fn upsert_yaml_section_scalar(
    document: &str,
    section_key: &str,
    scalar_key: &str,
    value: &str,
) -> String {
    let spans = yaml_line_spans(document);
    if let Some((section_start, section_end)) =
        find_yaml_section_bounds_local(document, section_key)
    {
        let mut entry_start = None;
        for (start, end) in spans
            .iter()
            .copied()
            .filter(|(start, _)| *start > section_start && *start < section_end)
        {
            let line = &document[start..end];
            if parse_yaml_nested_scalar_key(line).is_some_and(|key| key == scalar_key) {
                entry_start = Some((start, end));
                break;
            }
        }
        if let Some((start, end)) = entry_start {
            return format!(
                "{}  {}: {}\n{}",
                &document[..start],
                scalar_key,
                yaml_scalar_local(value),
                &document[end..]
            );
        }
        let insertion = format!("  {}: {}\n", scalar_key, yaml_scalar_local(value));
        return format!(
            "{}{}{}",
            &document[..section_end],
            insertion,
            &document[section_end..]
        );
    }
    let mut merged = document.to_string();
    if !merged.is_empty() && !merged.ends_with('\n') {
        merged.push('\n');
    }
    merged.push_str(section_key);
    merged.push_str(":\n");
    merged.push_str(&format!("  {}: {}\n", scalar_key, yaml_scalar_local(value)));
    merged
}

fn yaml_scalar_local(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn yaml_line_spans(text: &str) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut start = 0;
    for (index, byte) in text.as_bytes().iter().enumerate() {
        if *byte == b'\n' {
            spans.push((start, index + 1));
            start = index + 1;
        }
    }
    if start < text.len() {
        spans.push((start, text.len()));
    }
    spans
}

fn parse_yaml_top_level_key_local(line: &str) -> Option<&str> {
    if line.starts_with(' ') || line.starts_with('\t') {
        return None;
    }
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }
    trimmed.strip_suffix(':')
}

fn parse_yaml_nested_scalar_key(line: &str) -> Option<String> {
    if !line.starts_with("  ") || line.starts_with("    ") {
        return None;
    }
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }
    let (key, _) = trimmed.split_once(':')?;
    Some(key.trim_matches('\'').trim_matches('"').to_string())
}

fn find_yaml_section_bounds_local(existing: &str, section_key: &str) -> Option<(usize, usize)> {
    let spans = yaml_line_spans(existing);
    for (index, (start, _end)) in spans.iter().enumerate() {
        let line = &existing[*start..spans[index].1];
        if parse_yaml_top_level_key_local(line).is_some_and(|key| key == section_key) {
            for (next_start, next_end) in spans.iter().skip(index + 1) {
                let next_line = &existing[*next_start..*next_end];
                if parse_yaml_top_level_key_local(next_line).is_some() {
                    return Some((*start, *next_start));
                }
            }
            return Some((*start, existing.len()));
        }
    }
    None
}

fn render_hermes_profile_soul(base: Option<&str>, startup_instructions: &str) -> String {
    const BEGIN: &str = "<!-- AMAI MANAGED HERMES PROFILE STARTUP v1 -->";
    const END: &str = "<!-- /AMAI MANAGED HERMES PROFILE STARTUP v1 -->";
    let mut body = base.unwrap_or_default().trim().to_string();
    if !body.is_empty() {
        body.push_str("\n\n");
    }
    body.push_str(BEGIN);
    body.push('\n');
    body.push_str(
        "Этот managed block materialized Amai onboarding-ом и должен оставаться в project-bound Hermes profile.\n\n",
    );
    body.push_str(startup_instructions.trim());
    body.push('\n');
    body.push_str(END);
    body.push('\n');
    body
}

fn ensure_hermes_project_profile(
    repo_root: &Path,
    client_config_path: &Path,
    startup_instruction_path: &Path,
) -> Result<HermesProfileInstallSummary> {
    let profile_name = hermes_profile_id(repo_root);
    let profile_dir = hermes_profile_dir(&profile_name)?;
    let runtime_artifact_path = hermes_runtime_artifact_path(repo_root);
    let previous_active_profile = if runtime_artifact_path.is_file() {
        let content = fs::read_to_string(&runtime_artifact_path)
            .with_context(|| format!("failed to read {}", runtime_artifact_path.display()))?;
        serde_json::from_str::<HermesRuntimeArtifact>(&content)
            .with_context(|| format!("failed to parse {}", runtime_artifact_path.display()))?
            .previous_active_profile
    } else {
        read_hermes_active_profile()?
    };

    ensure_hermes_profile_dirs(&profile_dir)?;

    let default_home = hermes_default_home()?;
    let startup_instructions = fs::read_to_string(startup_instruction_path)
        .with_context(|| format!("failed to read {}", startup_instruction_path.display()))?;

    let base_config = fs::read_to_string(client_config_path)
        .with_context(|| format!("failed to read {}", client_config_path.display()))?;
    let updated_config = upsert_yaml_section_scalar(
        &base_config,
        "terminal",
        "cwd",
        &repo_root.display().to_string(),
    );
    fs::write(profile_dir.join("config.yaml"), updated_config.as_bytes()).with_context(|| {
        format!(
            "failed to write {}",
            profile_dir.join("config.yaml").display()
        )
    })?;

    copy_optional_file(&default_home.join(".env"), &profile_dir.join(".env"))?;
    copy_optional_file(
        &default_home.join("memories").join("MEMORY.md"),
        &profile_dir.join("memories").join("MEMORY.md"),
    )?;
    copy_optional_file(
        &default_home.join("memories").join("USER.md"),
        &profile_dir.join("memories").join("USER.md"),
    )?;
    copy_optional_dir_recursive(&default_home.join("skills"), &profile_dir.join("skills"))?;

    let base_soul = fs::read_to_string(default_home.join("SOUL.md")).ok();
    let rendered_soul = render_hermes_profile_soul(base_soul.as_deref(), &startup_instructions);
    fs::write(profile_dir.join("SOUL.md"), rendered_soul.as_bytes())
        .with_context(|| format!("failed to write {}", profile_dir.join("SOUL.md").display()))?;

    if let Some(parent) = runtime_artifact_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let runtime_artifact = HermesRuntimeArtifact {
        profile_name: profile_name.clone(),
        profile_dir: profile_dir.display().to_string(),
        previous_active_profile,
    };
    fs::write(
        &runtime_artifact_path,
        serde_json::to_string_pretty(&runtime_artifact)
            .context("failed to serialize Hermes runtime artifact")?,
    )
    .with_context(|| format!("failed to write {}", runtime_artifact_path.display()))?;
    write_hermes_active_profile(&profile_name)?;

    Ok(HermesProfileInstallSummary {
        profile_name,
        profile_dir,
        runtime_artifact_path,
    })
}

fn remove_hermes_project_profile(repo_root: &Path) -> Result<Option<ClientRuntimeInstallSummary>> {
    let runtime_artifact_path = hermes_runtime_artifact_path(repo_root);
    if !runtime_artifact_path.is_file() {
        return Ok(None);
    }
    let content = fs::read_to_string(&runtime_artifact_path)
        .with_context(|| format!("failed to read {}", runtime_artifact_path.display()))?;
    let artifact: HermesRuntimeArtifact = serde_json::from_str(&content)
        .with_context(|| format!("failed to parse {}", runtime_artifact_path.display()))?;
    let active_profile = read_hermes_active_profile()?;
    if active_profile == artifact.profile_name {
        let previous_profile_dir = if artifact.previous_active_profile == "default" {
            Some(hermes_default_home()?)
        } else {
            Some(hermes_profile_dir(&artifact.previous_active_profile)?)
        };
        let restore_to = if artifact.previous_active_profile == "default"
            || previous_profile_dir.is_some_and(|path| path.is_dir())
        {
            artifact.previous_active_profile.as_str()
        } else {
            "default"
        };
        write_hermes_active_profile(restore_to)?;
    }
    let profile_dir = PathBuf::from(&artifact.profile_dir);
    if profile_dir.is_dir() {
        fs::remove_dir_all(&profile_dir)
            .with_context(|| format!("failed to remove {}", profile_dir.display()))?;
    }
    fs::remove_file(&runtime_artifact_path)
        .with_context(|| format!("failed to remove {}", runtime_artifact_path.display()))?;
    Ok(Some(ClientRuntimeInstallSummary {
        status: "managed_hermes_profile_removed".to_string(),
        output_path: profile_dir,
        install_scope: "user_global".to_string(),
        reason: format!(
            "Hermes project profile `{}` removed and sticky default restored",
            artifact.profile_name
        ),
    }))
}

fn openclaw_list_agents(config_path: &Path) -> Result<Vec<OpenClawAgentListEntry>> {
    let output = openclaw_base_command(config_path)
        .arg("agents")
        .arg("list")
        .arg("--json")
        .output()
        .with_context(|| "failed to run openclaw agents list")?;
    if !output.status.success() {
        bail!(
            "openclaw agents list failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    serde_json::from_slice(&output.stdout).context("failed to parse openclaw agents list json")
}

fn openclaw_agent_exists(config_path: &Path, agent_id: &str) -> Result<bool> {
    Ok(openclaw_list_agents(config_path)?
        .iter()
        .any(|agent| agent.id == agent_id))
}

fn ensure_openclaw_project_agent(
    config_path: &Path,
    agent_id: &str,
    workspace_root: &Path,
) -> Result<OpenClawAgentAddSummary> {
    let workspace = workspace_root.display().to_string();
    let agents = openclaw_list_agents(config_path)?;
    if agents
        .iter()
        .any(|agent| agent.id == agent_id && agent.workspace.as_deref() == Some(workspace.as_str()))
    {
        return Ok(OpenClawAgentAddSummary {
            agent_id: agent_id.to_string(),
            _workspace: workspace,
        });
    }
    if agents.iter().any(|agent| agent.id == agent_id) {
        delete_openclaw_project_agent(config_path, agent_id)?;
    }
    let output = openclaw_base_command(config_path)
        .arg("agents")
        .arg("add")
        .arg(agent_id)
        .arg("--workspace")
        .arg(&workspace)
        .arg("--non-interactive")
        .arg("--json")
        .output()
        .with_context(|| "failed to run openclaw agents add")?;
    if !output.status.success() {
        bail!(
            "openclaw agents add failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    serde_json::from_slice(&output.stdout).context("failed to parse openclaw agents add json")
}

fn delete_openclaw_project_agent(config_path: &Path, agent_id: &str) -> Result<()> {
    let output = openclaw_base_command(config_path)
        .arg("agents")
        .arg("delete")
        .arg(agent_id)
        .arg("--force")
        .arg("--json")
        .output()
        .with_context(|| "failed to run openclaw agents delete")?;
    if output.status.success() {
        return Ok(());
    }
    bail!(
        "openclaw agents delete failed: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
}

fn startup_contract_for_workspace(
    workspace_root: &Path,
    helper_repo_root: &Path,
) -> Result<(Value, String)> {
    let mut contract = mcp::project_chat_startup_contract();
    if workspace_root != helper_repo_root {
        let helper_script = |name: &str| {
            helper_repo_root
                .join("scripts")
                .join(name)
                .display()
                .to_string()
        };
        contract["tool_runtime_reconcile"]["local_cli"]["shell_command"] =
            json!(helper_script("continuity_startup.sh"));
        contract["tool_runtime_reconcile"]["reconnect_helper"]["shell_helper_relative_path"] =
            json!(helper_script("reconnect_local.sh"));
        contract["runtime_state_artifact"]["inspection_fallback_cli"]["shell_command"] =
            json!(helper_script("continuity_startup_state.sh"));
        contract["live_client_budget_enforcement"]["guard_shell_command"] =
            json!(helper_script("client_budget_gate.sh"));
        contract["live_client_budget_enforcement"]["compact_diagnostics_shell_command"] =
            json!(helper_script("client_budget_root_cause.sh"));
        contract["live_client_budget_enforcement"]["target_control"]["shell_command"] =
            json!(helper_script("continuity_client_budget_target.sh"));
        contract["live_client_budget_enforcement"]["compact_chat_control"]["shell_command"] =
            json!(helper_script("continuity_compact_chat.sh"));
    }
    let sha256 = startup_contract_sha256(&contract)?;
    Ok((contract, sha256))
}

fn startup_contract_sha256(contract: &Value) -> Result<String> {
    let bytes = serde_json::to_vec(contract)
        .context("failed to serialize project_chat_startup contract")?;
    Ok(hex_sha256(&bytes))
}

fn hex_sha256(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(&mut hex, "{byte:02x}");
    }
    hex
}

fn install_scope_status(scope: &str) -> &'static str {
    match scope {
        "workspace_local" => "внутри текущего репозитория",
        "user_global" => "в профиле пользователя",
        "manual_generated" => "сгенерирован для ручного импорта",
        _ => "сгенерирован",
    }
}

fn detection_score(repo_root: &Path, home: &Path, target: &ClientTarget) -> Option<i64> {
    let mut score = 0_i64;
    if target
        .detect_env_vars
        .iter()
        .any(|key| env::var_os(key).is_some())
    {
        score += 1000;
    }
    if target
        .detect_workspace_markers
        .iter()
        .any(|marker| repo_root.join(marker).exists())
    {
        score += 100;
    }
    if target
        .detect_home_markers
        .iter()
        .any(|marker| home.join(marker).exists())
    {
        score += 10;
    }
    if score == 0 {
        return None;
    }
    Some(score + target.priority)
}

fn detection_reason(repo_root: &Path, home: &Path, target: &ClientTarget) -> Option<String> {
    if let Some(env_key) = target
        .detect_env_vars
        .iter()
        .find(|key| env::var_os(key).is_some())
    {
        return Some(format!("env_var:{env_key}"));
    }
    if let Some(marker) = target
        .detect_workspace_markers
        .iter()
        .find(|marker| repo_root.join(marker).exists())
    {
        return Some(format!("workspace_marker:{marker}"));
    }
    if let Some(marker) = target
        .detect_home_markers
        .iter()
        .find(|marker| home.join(marker).exists())
    {
        return Some(format!("home_marker:{marker}"));
    }
    None
}

fn maybe_backup_user_global(path: &Path, install_scope: &str) -> Result<Option<PathBuf>> {
    if install_scope != "user_global" || !path.is_file() {
        return Ok(None);
    }

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock error while creating backup name")?
        .as_secs();
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow!("failed to derive backup filename for {}", path.display()))?;
    let backup = path.with_file_name(format!("{file_name}.bak-{timestamp}"));
    fs::copy(path, &backup).with_context(|| {
        format!(
            "failed to create backup before modifying user-global config {}",
            path.display()
        )
    })?;
    Ok(Some(backup))
}

#[cfg(test)]
mod tests {
    use super::{
        InstallState, describe_client_surface, detection_score, ensure_hermes_project_profile,
        env_keys, expand_target_template, hermes_profile_id, inspect_startup_artifacts,
        install_scope_status, merge_managed_startup_block, remove_hermes_project_profile,
        render_agent_preflight_contract_artifact, render_agent_preflight_state_artifact,
        render_startup_agent_contract_artifact, render_startup_contract_artifact,
        render_startup_instructions, resolve_client_target, resolve_output_path,
        save_install_state, startup_agent_contract_artifact_path, startup_contract_artifact_path,
        startup_contract_sha256, strip_managed_startup_block, working_state_reason_summary,
    };
    use crate::continuity;
    use crate::mcp;
    use crate::working_state;
    use serde_json::{Value, json};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::{Mutex, OnceLock};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn hermes_env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn unique_test_dir(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock before epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("amai-{label}-{nanos}"))
    }

    #[test]
    fn env_keys_ignore_comments_and_blanks() {
        let keys = env_keys(
            r#"
# comment
AMI_STACK_NAME=amai

AMI_DEFAULT_RETRIEVAL_MODE=local_strict
"#,
        );
        assert!(keys.contains("AMI_STACK_NAME"));
        assert!(keys.contains("AMI_DEFAULT_RETRIEVAL_MODE"));
        assert_eq!(keys.len(), 2);
    }

    #[test]
    fn load_client_targets_manifest() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
        let target = resolve_client_target(repo, "vscode", false)
            .expect("vscode target must exist")
            .target;
        assert_eq!(target.install_scope, "workspace_local");
        assert_eq!(target.display_name, "VS Code");
        let startup = target
            .startup_instructions
            .expect("vscode startup instructions must be configured");
        assert_eq!(startup.mode, "managed_workspace_file");
        assert_eq!(startup.format, "vscode_instructions_md");
        let codex = resolve_client_target(repo, "codex", false)
            .expect("codex target must exist")
            .target;
        let codex_startup = codex
            .startup_instructions
            .expect("codex startup instructions must be configured");
        assert_eq!(codex_startup.mode, "managed_append_block");
        assert_eq!(codex_startup.install_scope, "workspace_local");
        let hermes = resolve_client_target(repo, "hermes", false)
            .expect("hermes target must exist")
            .target;
        let hermes_startup = hermes
            .startup_instructions
            .expect("hermes startup instructions must be configured");
        assert_eq!(hermes.default_output, "${home}/.hermes/config.yaml");
        assert_eq!(hermes_startup.mode, "managed_workspace_file");
        assert_eq!(hermes_startup.install_scope, "workspace_local");
        let openclaw = resolve_client_target(repo, "openclaw", false)
            .expect("openclaw target must exist")
            .target;
        let openclaw_startup = openclaw
            .startup_instructions
            .expect("openclaw startup instructions must be configured");
        assert_eq!(openclaw.default_output, "${home}/.openclaw/openclaw.json");
        assert_eq!(openclaw_startup.mode, "managed_workspace_file");
        assert_eq!(openclaw_startup.install_scope, "workspace_local");
    }

    #[test]
    fn resolves_default_output_paths() {
        let repo = Path::new("/tmp/amai");
        let home = Path::new("/tmp/home");
        assert_eq!(
            expand_target_template("${repo_root}/.vscode/mcp.json", repo, home),
            repo.join(".vscode/mcp.json")
        );
        assert_eq!(
            expand_target_template("${home}/.codex/config.toml", repo, home),
            home.join(".codex/config.toml")
        );
    }

    #[test]
    fn describe_client_surface_reports_managed_and_manual_targets() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
        let codex = describe_client_surface(repo, Some("codex")).expect("codex surface");
        assert_eq!(codex["client_key"], json!("codex"));
        assert_eq!(codex["display_name"], json!("Codex"));
        assert_eq!(
            codex["startup_instruction_mode"],
            json!("managed_append_block")
        );
        assert_eq!(
            codex["startup_instruction_path"],
            json!(repo.join("AGENTS.md").display().to_string())
        );
        assert_eq!(
            codex["reconnect_shell_command"],
            json!("./scripts/reconnect_local.sh --client codex")
        );
        assert_eq!(
            codex["reconnect_bootstrap_command"],
            json!("./scripts/amai_exec.sh bootstrap reconnect --client codex --yes")
        );

        let generic = describe_client_surface(repo, Some("generic")).expect("generic surface");
        assert_eq!(generic["client_key"], json!("generic"));
        assert_eq!(
            generic["startup_instruction_mode"],
            json!("manual_snippet_only")
        );
        assert!(
            generic["startup_instruction_path"]
                .as_str()
                .is_some_and(|value| value.ends_with("tmp/onboarding/generic-amai-startup.md"))
        );
        assert!(
            generic["fresh_chat_assist_summary"].as_str().is_some_and(
                |value| value.contains("./scripts/reconnect_local.sh --client generic")
            )
        );
        assert_eq!(
            generic["delivery_surface_assist_summary"],
            generic["fresh_chat_assist_summary"]
        );

        let hermes = describe_client_surface(repo, Some("hermes")).expect("hermes surface");
        assert_eq!(hermes["client_key"], json!("hermes"));
        assert_eq!(hermes["display_name"], json!("Hermes"));
        assert_eq!(
            hermes["startup_instruction_mode"],
            json!("managed_workspace_file")
        );
        assert!(
            hermes["config_output_path"]
                .as_str()
                .is_some_and(|value| value.ends_with(".hermes/config.yaml"))
        );
        assert_eq!(
            hermes["delivery_surface_assist_summary"],
            hermes["fresh_chat_assist_summary"]
        );
        assert!(
            hermes["startup_instruction_path"]
                .as_str()
                .is_some_and(|value| value.ends_with(".hermes.md"))
        );

        let openclaw = describe_client_surface(repo, Some("openclaw")).expect("openclaw surface");
        assert_eq!(openclaw["client_key"], json!("openclaw"));
        assert_eq!(openclaw["display_name"], json!("OpenClaw"));
        assert_eq!(
            openclaw["startup_instruction_mode"],
            json!("managed_workspace_file")
        );
        assert!(
            openclaw["config_output_path"]
                .as_str()
                .is_some_and(|value| value.ends_with(".openclaw/openclaw.json"))
        );
        assert!(
            openclaw["startup_instruction_path"]
                .as_str()
                .is_some_and(|value| value.ends_with(".openclaw/AGENTS.md"))
        );
        assert!(
            openclaw["client_runtime_agent_id"]
                .as_str()
                .is_some_and(|value| value.starts_with("amai-"))
        );
        assert!(
            openclaw["client_runtime_workspace_path"]
                .as_str()
                .is_some_and(|value| value.ends_with(".openclaw"))
        );
    }

    #[test]
    fn reports_install_scope_statuses() {
        assert_eq!(
            install_scope_status("workspace_local"),
            "внутри текущего репозитория"
        );
        assert_eq!(
            install_scope_status("user_global"),
            "в профиле пользователя"
        );
        assert_eq!(
            install_scope_status("manual_generated"),
            "сгенерирован для ручного импорта"
        );
    }

    #[test]
    fn hermes_runtime_profile_install_and_remove_manage_sticky_default() {
        let _guard = hermes_env_lock().lock().expect("Hermes env lock poisoned");
        let previous_home = std::env::var_os("HOME");
        let home = unique_test_dir("hermes-runtime-profile");
        let repo_root = home.join("repo");
        let startup_path = repo_root.join(".hermes.md");
        let runtime_artifact_path = repo_root
            .join(".amai")
            .join("onboarding")
            .join("hermes-profile-runtime.json");
        let client_config_path = home.join(".hermes").join("config.yaml");
        let skills_src = home.join(".hermes").join("skills");
        let active_profile_path = home.join(".hermes").join("active_profile");
        let managed_profile_name = hermes_profile_id(&repo_root);
        let managed_profile_dir = home
            .join(".hermes")
            .join("profiles")
            .join(&managed_profile_name);

        fs::create_dir_all(repo_root.join(".amai").join("onboarding"))
            .expect("failed to create repo onboarding dir");
        fs::create_dir_all(client_config_path.parent().expect("config parent"))
            .expect("failed to create Hermes config dir");
        fs::create_dir_all(home.join(".hermes").join("memories"))
            .expect("failed to create Hermes memories dir");
        fs::create_dir_all(&skills_src).expect("failed to create Hermes skills dir");
        fs::write(
            &client_config_path,
            "model:\n  default: gemma4:e4b\nmcp_servers:\n  amai:\n    command: amai\n",
        )
        .expect("failed to write Hermes config");
        fs::write(
            startup_path
                .parent()
                .expect("startup parent")
                .join(".hermes.md"),
            "<!-- AMAI MANAGED STARTUP INSTRUCTIONS v1 -->\namai_continuity_startup\n",
        )
        .expect("failed to write startup instructions");
        fs::write(home.join(".hermes").join("SOUL.md"), "Base Hermes soul\n")
            .expect("failed to write base SOUL");
        fs::write(
            home.join(".hermes").join("memories").join("MEMORY.md"),
            "remember this\n",
        )
        .expect("failed to write MEMORY");
        fs::write(
            home.join(".hermes").join("memories").join("USER.md"),
            "prefer concise answers\n",
        )
        .expect("failed to write USER");
        fs::write(skills_src.join("sample.txt"), "skill payload\n")
            .expect("failed to write sample skill");
        fs::write(&active_profile_path, "default\n").expect("failed to seed active profile");

        unsafe { std::env::set_var("HOME", &home) };
        let install = ensure_hermes_project_profile(&repo_root, &client_config_path, &startup_path)
            .expect("failed to install Hermes profile");

        assert_eq!(install.profile_name, managed_profile_name);
        assert_eq!(install.profile_dir, managed_profile_dir);
        assert!(install.runtime_artifact_path.is_file());
        assert_eq!(
            fs::read_to_string(&active_profile_path).expect("active profile"),
            format!("{}\n", install.profile_name)
        );
        let managed_config =
            fs::read_to_string(managed_profile_dir.join("config.yaml")).expect("managed config");
        assert!(managed_config.contains("mcp_servers:"));
        assert!(managed_config.contains("  amai:"));
        assert!(managed_config.contains("terminal:"));
        assert!(managed_config.contains(&format!("  cwd: '{}'", repo_root.display().to_string())));
        let managed_soul =
            fs::read_to_string(managed_profile_dir.join("SOUL.md")).expect("managed SOUL");
        assert!(managed_soul.contains("AMAI MANAGED HERMES PROFILE STARTUP v1"));
        assert!(managed_soul.contains("amai_continuity_startup"));
        assert!(managed_soul.contains("Base Hermes soul"));
        assert_eq!(
            fs::read_to_string(managed_profile_dir.join("memories").join("MEMORY.md"))
                .expect("managed MEMORY"),
            "remember this\n"
        );
        assert_eq!(
            fs::read_to_string(managed_profile_dir.join("memories").join("USER.md"))
                .expect("managed USER"),
            "prefer concise answers\n"
        );
        assert_eq!(
            fs::read_to_string(managed_profile_dir.join("skills").join("sample.txt"))
                .expect("managed skill"),
            "skill payload\n"
        );
        let runtime_artifact: Value = serde_json::from_str(
            &fs::read_to_string(&runtime_artifact_path).expect("runtime artifact"),
        )
        .expect("runtime artifact json");
        assert_eq!(
            runtime_artifact["profile_name"],
            json!(install.profile_name.clone())
        );
        assert_eq!(
            runtime_artifact["previous_active_profile"],
            json!("default")
        );

        let remove_summary = remove_hermes_project_profile(&repo_root)
            .expect("remove should succeed")
            .expect("remove summary");
        assert_eq!(remove_summary.status, "managed_hermes_profile_removed");
        assert!(!active_profile_path.exists());
        assert!(!managed_profile_dir.exists());
        assert!(!runtime_artifact_path.exists());

        if let Some(previous_home) = previous_home {
            unsafe { std::env::set_var("HOME", previous_home) };
        } else {
            unsafe { std::env::remove_var("HOME") };
        }
        fs::remove_dir_all(&home).expect("failed to remove test home");
    }

    #[test]
    fn resolve_output_path_prefers_explicit_path() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
        let target = resolve_client_target(repo, "vscode", false).unwrap().target;
        let explicit = repo.join("custom/mcp.json");
        assert_eq!(
            resolve_output_path(repo, &target, Some(&explicit)).unwrap(),
            explicit
        );
    }

    #[test]
    fn resolve_client_target_auto_prefers_workspace_marker() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
        let resolution = resolve_client_target(repo, "auto", false).unwrap();
        assert_eq!(resolution.client_key, "vscode");
        assert!(resolution.auto_selected);
        assert!(resolution.reason.starts_with("workspace_marker:"));
    }

    #[test]
    fn detection_score_requires_some_signal() {
        let target = super::ClientTarget {
            display_name: "Example".to_string(),
            default_output: "${repo_root}/tmp/example.json".to_string(),
            install_scope: "workspace_local".to_string(),
            priority: 50,
            detect_env_vars: Vec::new(),
            detect_workspace_markers: vec!["missing-marker".to_string()],
            detect_home_markers: Vec::new(),
            startup_instructions: None,
        };
        let repo = PathBuf::from("/tmp/amai-nonexistent");
        let home = PathBuf::from("/tmp/amai-home-nonexistent");
        assert!(detection_score(&repo, &home, &target).is_none());
    }

    #[test]
    fn renders_vscode_startup_instructions_with_repo_root() {
        let repo = Path::new("/tmp/amai");
        let text =
            render_startup_instructions(repo, repo, "VS Code", "vscode", "vscode_instructions_md")
                .expect("startup instructions must render");
        assert!(text.contains("AMAI MANAGED STARTUP INSTRUCTIONS v1"));
        assert!(text.contains(
            "Перед первым содержательным ответом в новом или resumed чате и дальше перед каждым следующим содержательным ответом:"
        ));
        assert!(text.contains("/AMAI MANAGED STARTUP INSTRUCTIONS v1"));
        assert!(text.contains("amai_continuity_startup"));
        assert!(text.contains("/tmp/amai"));
        assert!(text.contains("pinned source-of-truth"));
        assert!(text.contains("continuity_startup_summary"));
        assert!(text.contains("required_summary_fields"));
        assert!(text.contains("restored_obligations"));
        assert!(text.contains("startup_next_action"));
        assert!(text.contains("execctl_active_lease"));
        assert!(text.contains("lease_owner_state"));
        assert!(text.contains("previous_session_owner"));
        assert!(text.contains("resume_required_return_task"));
        assert!(
            text.contains(
                startup_contract_artifact_path(repo)
                    .display()
                    .to_string()
                    .as_str()
            )
        );
        assert!(
            text.contains(
                startup_agent_contract_artifact_path(repo)
                    .display()
                    .to_string()
                    .as_str()
            )
        );
        assert!(text.contains("machine-readable startup contract"));
        assert!(text.contains("compact agent contract"));
        assert!(text.contains("workspace_contract_required_before_tool_call = true"));
        assert!(text.contains("missing_or_unreadable_fail_closed = true"));
        assert!(text.contains("sha256_mismatch_fail_closed = true"));
        assert!(text.contains(".amai/continuity/project-chat-startup-state.json"));
        assert!(text.contains(
            "`workspace_runtime_state_artifact_version` должен быть `workspace-startup-runtime-state-v4`"
        ));
        assert!(text.contains("его пишет `amai_continuity_startup`"));
        assert!(text.contains("он обязан нести `continuity_startup_summary`"));
        assert!(text.contains("startup_execution_gate"));
        assert!(text.contains("startup_execution_gate.must_follow_startup_next_action != true"));
        assert!(text.contains("startup_execution_gate.unrelated_work_allowed != false"));
        assert!(text.contains("startup_execution_gate.must_read_prompt_text_before_reply != true"));
        assert!(text.contains("startup_execution_gate.no_silent_drop != true"));
        assert!(text.contains(
            "startup_execution_gate.required_action_kind_when_resume_required == \"resume_required_return_task\""
        ));
        assert!(
            text.contains("startup_next_action.action_kind == \"resume_required_return_task\"")
        );
        assert!(text.contains("gate_semantics_consistent != true"));
        assert!(text.contains("gate_semantics_consistent_true_required = true"));
        assert!(text.contains("./scripts/continuity_startup_state.sh --repo-root"));
        assert!(text.contains("tool_execution_failed"));
        assert!(text.contains("no continuity import found for"));
        assert!(text.contains("Transport closed"));
        assert!(text.contains("./scripts/continuity_startup.sh --repo-root"));
        assert!(text.contains("requires_namespace_argument = true"));
        assert!(text.contains("stale_embedded_mcp_session"));
        assert!(text.contains("local_cli_success_replaces_transport_failure = true"));
        assert!(text.contains("must_request_mcp_reconnect_after_local_success = true"));
        assert!(text.contains("must_continue_from_local_startup_payload = true"));
        assert!(text.contains("./scripts/reconnect_local.sh --client vscode"));
        assert!(text.contains("./scripts/amai_exec.sh bootstrap reconnect --client vscode --yes"));
        let expected_sha = startup_contract_sha256(&mcp::project_chat_startup_contract())
            .expect("startup contract hash");
        assert!(text.contains(expected_sha.as_str()));
        assert!(text.contains("previous_session_owner_must_follow_startup_next_action = true"));
        assert!(text.contains("no_silent_drop = true"));
        assert!(text.contains("./scripts/client_budget_gate.sh"));
        assert!(text.contains("must_check_before_each_substantive_reply = true"));
        assert!(text.contains("continuity_write_exempt_from_reply_guard = true"));
        assert!(text.contains("continuity_write_required_before_rotate = true"));
        assert!(
            text.contains("continuity import, continuity handoff, observe /api/continuity-handoff")
        );
        assert!(text.contains("--enforce-reply-gate"));
        assert!(text.contains("--enforce-online-reply-prefix"));
        assert!(text.contains("guard_enforcement_exit_on_blocking = true"));
        assert!(
            text.contains("required_reply_prefix_source = personal_agent_online_limit_contour")
        );
        assert!(text.contains("required_reply_prefix_non_empty = true"));
        assert!(text.contains("reply_prefix_preflight_blocks_substantive_reply = true"));
        assert!(
            text.contains("output_prefix_enforcement_mode = instruction_preflight_fail_closed")
        );
        assert!(text.contains("output_prefix_host_enforced = false"));
        assert!(text.contains("./scripts/client_budget_root_cause.sh"));
        assert!(text.contains("must_prefer_compact_diagnostics_over_full_snapshot = true"));
        assert!(text.contains("client_budget_reply_gate.reply_execution_gate"));
        assert!(text.contains("Gate version pinned: `client-reply-budget-gate-v1`"));
        assert!(text.contains("reply_execution_gate.reply_prefix"));
        assert!(text.contains("Начинать user-visible reply можно только если"));
        assert!(text.contains("matching `^экономия_(0|10|20|30|40|50|60|70|80|90)%$`"));
        assert!(text.contains("./scripts/continuity_client_budget_target.sh --repo-root"));
        assert!(text.contains("Пример exact chat-команды: `экономия_50%`"));
        assert!(text.contains("точную команду `компакт_чат`"));
        assert!(text.contains("./scripts/continuity_compact_chat.sh --repo-root"));
        assert!(text.contains("`prompt_text` и `operator_notice`"));
        assert!(text.contains(
            "open_clean_chat_surface_and_inject_prompt_text_if_launch_bridge_unavailable"
        ));
        assert!(text.contains("reply_budget_mode == \"compact_high_signal\""));
        assert!(text.contains("reply_budget_contract"));
        assert!(text.contains("contract_version = \"client-reply-budget-v1\""));
        assert!(text.contains("direct answer first"));
        assert!(text.contains("no unrequested recap"));
        assert!(text.contains("no repeated known context"));
        assert!(text.contains("stale старше `10` секунд"));
        assert!(text.contains("stale_guard_requires_refresh = true"));
        assert!(text.contains("Client-budget blocked reply mechanism removed"));
        assert!(text.contains("reply_blocking_removed = true"));
        assert!(text.contains("tool_turn_blocking_removed = true"));
        assert!(text.contains("User-visible blocked wait template использовать запрещено"));
        assert!(text.contains("amai_context_pack"));
        assert!(text.contains("сожми текущий чат сейчас"));
        assert!(text.contains("сожми текущий чат"));
        assert!(text.contains("current normalized same-thread advisory labels"));
        assert!(
            text.contains(
                "non-binding human-readable snapshot канонического shared advisory source"
            )
        );
        assert!(!text.contains("новый чат нужен сейчас"));
        assert!(!text.contains("новый чат рекомендован"));
        assert!(text.contains("advisory/compact pressure signal"));
        assert!(text.contains("full_scale_client_truth_required = true"));
        assert!(text.contains("внутренним Amai-slice"));
        assert!(text.len() < 9000);
    }

    #[test]
    fn renders_machine_readable_startup_contract_artifact() {
        let repo = Path::new("/tmp/amai");
        let (text, sha256) =
            render_startup_contract_artifact(repo, repo).expect("startup contract must render");
        let payload: Value = serde_json::from_str(&text).expect("startup contract json");
        assert_eq!(
            payload["artifact_version"],
            json!("workspace-startup-contract-v1")
        );
        assert_eq!(payload["repo_root"], json!("/tmp/amai"));
        assert_eq!(payload["startup_contract_sha256"], json!(sha256));
        assert_eq!(
            payload["startup_contract"]["artifact_enforcement"]["workspace_contract_relative_path"],
            json!(".amai/onboarding/project-chat-startup-contract.json")
        );
        assert_eq!(
            payload["startup_contract"]["artifact_enforcement"]["missing_or_unreadable_fail_closed"],
            json!(true)
        );
        assert_eq!(
            payload["recommended_startup_call"]["arguments"]["repo_root"],
            json!("/tmp/amai")
        );
        assert_eq!(
            payload["startup_contract"]["resume_enforcement"]["required_action_kind_when_resume_required"],
            json!("resume_required_return_task")
        );
        assert_eq!(
            payload["startup_contract"]["live_client_budget_enforcement"]["blocking_reply_contract_field"],
            json!("blocking_reply_contract")
        );
        assert_eq!(
            payload["startup_contract"]["live_client_budget_enforcement"]["blocking_reply_contract_version"],
            json!(working_state::CLIENT_BUDGET_BLOCKING_REPLY_CONTRACT_VERSION)
        );
        assert_eq!(
            payload["startup_contract"]["live_client_budget_enforcement"]["blocking_reply_response_kind"],
            Value::Null
        );
        assert_eq!(
            payload["startup_contract"]["live_client_budget_enforcement"]["reply_blocking_removed"],
            json!(true)
        );
        assert_eq!(
            payload["startup_contract"]["live_client_budget_enforcement"]["tool_turn_blocking_removed"],
            json!(true)
        );
        assert_eq!(
            payload["startup_contract"]["live_client_budget_enforcement"]["blocking_reply_max_sentences"],
            json!(0)
        );
        assert_eq!(
            payload["startup_contract"]["runtime_state_artifact"]["workspace_runtime_state_relative_path"],
            json!(".amai/continuity/project-chat-startup-state.json")
        );
        assert_eq!(
            payload["startup_contract"]["runtime_state_artifact"]["workspace_runtime_state_artifact_version"],
            json!("workspace-startup-runtime-state-v4")
        );
        assert_eq!(
            payload["startup_contract"]["runtime_state_artifact"]["source_summary_field"],
            json!("continuity_startup_summary")
        );
        assert_eq!(
            payload["startup_contract"]["runtime_state_artifact"]["startup_execution_gate_field"],
            json!("startup_execution_gate")
        );
        assert_eq!(
            payload["startup_contract"]["runtime_state_artifact"]["inspection_fallback_cli"]["command"],
            json!("continuity startup-state")
        );
        assert_eq!(
            payload["startup_contract"]["runtime_state_artifact"]["inspection_fallback_cli"]["shell_command"],
            json!("./scripts/continuity_startup_state.sh")
        );
        assert_eq!(
            payload["startup_contract"]["runtime_state_artifact"]["gate_semantics_consistent_field"],
            json!("gate_semantics_consistent")
        );
        assert_eq!(
            payload["startup_contract"]["runtime_state_artifact"]["gate_semantics_consistent_true_required"],
            json!(true)
        );
        assert_eq!(
            payload["startup_contract"]["startup_execution_gate_enforcement"]["gate_field"],
            json!("startup_execution_gate")
        );
        assert_eq!(
            payload["startup_contract"]["startup_execution_gate_enforcement"]["must_follow_field"],
            json!("must_follow_startup_next_action")
        );
        assert_eq!(
            payload["startup_contract"]["startup_execution_gate_enforcement"]["unrelated_work_allowed_field"],
            json!("unrelated_work_allowed")
        );
        assert_eq!(
            payload["startup_contract"]["startup_execution_gate_enforcement"]["must_read_prompt_text_before_reply_field"],
            json!("must_read_prompt_text_before_reply")
        );
        assert_eq!(
            payload["startup_contract"]["startup_execution_gate_enforcement"]["required_action_kind_field"],
            json!("required_action_kind_when_resume_required")
        );
        assert_eq!(
            payload["startup_contract"]["startup_execution_gate_enforcement"]["no_silent_drop_field"],
            json!("no_silent_drop")
        );
        assert_eq!(
            payload["startup_contract"]["startup_execution_gate_enforcement"]["blocking_true_requires_must_follow"],
            json!(true)
        );
        assert_eq!(
            payload["startup_contract"]["startup_execution_gate_enforcement"]["blocking_true_blocks_unrelated_work"],
            json!(true)
        );
        assert_eq!(
            payload["startup_contract"]["startup_execution_gate_enforcement"]["must_follow_true_blocks_unrelated_work"],
            json!(true)
        );
        assert_eq!(
            payload["startup_contract"]["startup_execution_gate_enforcement"]["required_action_kind_resume_required_value"],
            json!("resume_required_return_task")
        );
        assert_eq!(
            payload["startup_contract"]["contract_version"],
            json!("continuity-startup-contract-v19")
        );
        assert_eq!(
            payload["startup_contract"]["tool_runtime_reconcile"]["error_class"],
            json!("tool_execution_failed")
        );
        assert_eq!(
            payload["startup_contract"]["tool_runtime_reconcile"]["error_detail_contains"],
            json!("no continuity import found for")
        );
        assert_eq!(
            payload["startup_contract"]["tool_runtime_reconcile"]["transport_error_detail_contains"],
            json!("Transport closed")
        );
        assert_eq!(
            payload["startup_contract"]["tool_runtime_reconcile"]["local_cli"]["command"],
            json!("continuity startup")
        );
        assert_eq!(
            payload["startup_contract"]["tool_runtime_reconcile"]["local_cli"]["shell_command"],
            json!("./scripts/continuity_startup.sh")
        );
        assert_eq!(
            payload["startup_contract"]["tool_runtime_reconcile"]["local_cli_success_classification"],
            json!("stale_embedded_mcp_session")
        );
        assert_eq!(
            payload["startup_contract"]["tool_runtime_reconcile"]["local_cli_success_replaces_transport_failure"],
            json!(true)
        );
        assert_eq!(
            payload["startup_contract"]["tool_runtime_reconcile"]["must_request_mcp_reconnect_after_local_success"],
            json!(true)
        );
        assert_eq!(
            payload["startup_contract"]["tool_runtime_reconcile"]["reconnect_helper"]["shell_helper_relative_path"],
            json!("./scripts/reconnect_local.sh")
        );
        assert_eq!(
            payload["startup_contract"]["purpose"],
            json!(
                "project-scoped continuity restore plus live client-budget discipline before each substantive reply on a new, resumed, or ongoing work surface"
            )
        );
        assert_eq!(
            payload["startup_contract"]["live_client_budget_enforcement"]["guard_command"],
            json!("observe client-budget-gate")
        );
        assert_eq!(
            payload["startup_contract"]["live_client_budget_enforcement"]["guard_shell_command"],
            json!("./scripts/client_budget_gate.sh")
        );
        assert_eq!(
            payload["startup_contract"]["live_client_budget_enforcement"]["reply_execution_gate_field"],
            json!("reply_execution_gate")
        );
        assert_eq!(
            payload["startup_contract"]["live_client_budget_enforcement"]["reply_execution_gate_version"],
            json!("client-reply-budget-gate-v1")
        );
        assert_eq!(
            payload["startup_contract"]["live_client_budget_enforcement"]["reply_prefix_field"],
            json!("reply_prefix")
        );
        assert_eq!(
            payload["startup_contract"]["live_client_budget_enforcement"]["reply_prefix_enforcement_flag"],
            json!("--enforce-online-reply-prefix")
        );
        assert_eq!(
            payload["startup_contract"]["live_client_budget_enforcement"]["required_reply_prefix_source"],
            json!("personal_agent_online_limit_contour")
        );
        assert_eq!(
            payload["startup_contract"]["live_client_budget_enforcement"]["required_reply_prefix_non_empty"],
            json!(true)
        );
        assert_eq!(
            payload["startup_contract"]["live_client_budget_enforcement"]["reply_prefix_preflight_blocks_substantive_reply"],
            json!(true)
        );
        assert_eq!(
            payload["startup_contract"]["live_client_budget_enforcement"]["output_prefix_enforcement_mode"],
            json!("instruction_preflight_fail_closed")
        );
        assert_eq!(
            payload["startup_contract"]["live_client_budget_enforcement"]["output_prefix_host_enforced"],
            json!(false)
        );
        assert_eq!(
            payload["startup_contract"]["live_client_budget_enforcement"]["reply_budget_mode_field"],
            json!("reply_budget_mode")
        );
        assert_eq!(
            payload["startup_contract"]["live_client_budget_enforcement"]["reply_budget_contract_field"],
            json!("reply_budget_contract")
        );
        assert_eq!(
            payload["startup_contract"]["live_client_budget_enforcement"]["compact_reply_mode_value"],
            json!(working_state::CLIENT_REPLY_BUDGET_MODE_COMPACT_HIGH_SIGNAL)
        );
        assert_eq!(
            payload["startup_contract"]["live_client_budget_enforcement"]["compact_reply_contract_version"],
            json!(working_state::CLIENT_REPLY_BUDGET_CONTRACT_VERSION)
        );
        assert_eq!(
            payload["startup_contract"]["live_client_budget_enforcement"]["compact_diagnostics_command"],
            json!("observe client-budget-root-cause")
        );
        assert_eq!(
            payload["startup_contract"]["live_client_budget_enforcement"]["compact_diagnostics_shell_command"],
            json!("./scripts/client_budget_root_cause.sh")
        );
        assert_eq!(
            payload["startup_contract"]["live_client_budget_enforcement"]["must_prefer_compact_diagnostics_over_full_snapshot"],
            json!(true)
        );
        assert_eq!(
            payload["startup_contract"]["live_client_budget_enforcement"]["guard_enforcement_flag"],
            json!("--enforce-reply-gate")
        );
        assert_eq!(
            payload["startup_contract"]["live_client_budget_enforcement"]["guard_enforcement_exit_on_blocking"],
            json!(true)
        );
        assert_eq!(
            payload["startup_contract"]["live_client_budget_enforcement"]["must_check_before_each_substantive_reply"],
            json!(true)
        );
        assert_eq!(
            payload["startup_contract"]["live_client_budget_enforcement"]["continuity_write_exempt_from_reply_guard"],
            json!(true)
        );
        assert_eq!(
            payload["startup_contract"]["live_client_budget_enforcement"]["continuity_write_required_before_rotate"],
            json!(true)
        );
        assert_eq!(
            payload["startup_contract"]["live_client_budget_enforcement"]["continuity_write_operations"],
            json!([
                "continuity import",
                "continuity handoff",
                "observe /api/continuity-handoff"
            ])
        );
        assert_eq!(
            payload["startup_contract"]["live_client_budget_enforcement"]["max_guard_age_seconds"],
            json!(10)
        );
        assert_eq!(
            payload["startup_contract"]["live_client_budget_enforcement"]["stale_guard_requires_refresh"],
            json!(true)
        );
        assert_eq!(
            payload["startup_contract"]["live_client_budget_enforcement"]["target_control"]["exact_chat_command_pattern"],
            json!("^экономия_(0|10|20|30|40|50|60|70|80|90)%$")
        );
        assert_eq!(
            payload["startup_contract"]["live_client_budget_enforcement"]["target_control"]["cli_command"],
            json!("continuity client-budget-target")
        );
        assert_eq!(
            payload["startup_contract"]["live_client_budget_enforcement"]["target_control"]["shell_command"],
            json!("./scripts/continuity_client_budget_target.sh")
        );
        assert_eq!(
            payload["startup_contract"]["live_client_budget_enforcement"]["compact_chat_control"]["exact_chat_command"],
            json!(continuity::CLIENT_BUDGET_COMPACT_CHAT_COMMAND)
        );
        assert_eq!(
            payload["startup_contract"]["live_client_budget_enforcement"]["compact_chat_control"]["cli_command"],
            json!("continuity compact-chat")
        );
        assert_eq!(
            payload["startup_contract"]["live_client_budget_enforcement"]["compact_chat_control"]["shell_command"],
            json!("./scripts/continuity_compact_chat.sh")
        );
        assert_eq!(
            payload["startup_contract"]["live_client_budget_enforcement"]["compact_chat_control"]["required_host_action"],
            json!("open_clean_chat_surface_and_inject_prompt_text_if_launch_bridge_unavailable")
        );
    }

    #[test]
    fn renders_compact_startup_agent_contract_artifact() {
        let repo = Path::new("/tmp/amai");
        let (full_text, sha256) =
            render_startup_contract_artifact(repo, repo).expect("startup contract must render");
        let text = render_startup_agent_contract_artifact(repo, repo)
            .expect("startup agent contract must render");
        let payload: Value = serde_json::from_str(&text).expect("startup agent contract json");

        assert_eq!(
            payload["artifact_version"],
            json!("workspace-startup-agent-contract-v1")
        );
        assert_eq!(
            payload["full_startup_contract_relative_path"],
            json!(".amai/onboarding/project-chat-startup-contract.json")
        );
        assert_eq!(payload["full_startup_contract_sha256"], json!(sha256));
        assert_eq!(
            payload["runtime_state_artifact"]["workspace_runtime_state_artifact_version"],
            json!("workspace-startup-runtime-state-v4")
        );
        assert_eq!(
            payload["compact_runtime_pointers"]["compact_diagnostics_command"],
            json!("observe client-budget-root-cause")
        );
        assert_eq!(
            payload["compact_runtime_pointers"]["compact_diagnostics_shell_command"],
            json!("./scripts/client_budget_root_cause.sh")
        );
        assert_eq!(
            payload["compact_runtime_pointers"]["reconcile_local_cli_shell_command"],
            json!("./scripts/continuity_startup.sh")
        );
        assert_eq!(
            payload["compact_runtime_pointers"]["startup_state_fallback_shell_command"],
            json!("./scripts/continuity_startup_state.sh")
        );
        assert_eq!(
            payload["compact_runtime_pointers"]["guard_command"],
            json!("observe client-budget-gate")
        );
        assert_eq!(
            payload["compact_runtime_pointers"]["guard_shell_command"],
            json!("./scripts/client_budget_gate.sh")
        );
        assert_eq!(
            payload["compact_runtime_pointers"]["reply_prefix_field"],
            json!("reply_prefix")
        );
        assert_eq!(
            payload["compact_runtime_pointers"]["reply_prefix_enforcement_flag"],
            json!("--enforce-online-reply-prefix")
        );
        assert_eq!(
            payload["compact_runtime_pointers"]["required_reply_prefix_source"],
            json!("personal_agent_online_limit_contour")
        );
        assert_eq!(
            payload["compact_runtime_pointers"]["client_budget_target_exact_chat_command_pattern"],
            json!("^экономия_(0|10|20|30|40|50|60|70|80|90)%$")
        );
        assert_eq!(
            payload["compact_runtime_pointers"]["client_budget_target_cli_command"],
            json!("continuity client-budget-target")
        );
        assert_eq!(
            payload["compact_runtime_pointers"]["client_budget_target_shell_command"],
            json!("./scripts/continuity_client_budget_target.sh")
        );
        assert_eq!(
            payload["compact_runtime_pointers"]["client_budget_compact_chat_exact_chat_command"],
            json!(continuity::CLIENT_BUDGET_COMPACT_CHAT_COMMAND)
        );
        assert_eq!(
            payload["compact_runtime_pointers"]["client_budget_compact_chat_cli_command"],
            json!("continuity compact-chat")
        );
        assert_eq!(
            payload["compact_runtime_pointers"]["client_budget_compact_chat_shell_command"],
            json!("./scripts/continuity_compact_chat.sh")
        );
        assert_eq!(
            payload["compact_runtime_pointers"]["client_budget_compact_chat_required_host_action"],
            json!("open_clean_chat_surface_and_inject_prompt_text_if_launch_bridge_unavailable")
        );
        assert!(
            text.len() < full_text.len(),
            "startup agent contract must be smaller than full contract"
        );
    }

    #[test]
    fn renders_machine_readable_agent_preflight_contract_artifact() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
        let (text, sha256) = render_agent_preflight_contract_artifact(repo, repo)
            .expect("agent preflight contract must render");
        let payload: Value = serde_json::from_str(&text).expect("agent preflight json");
        assert_eq!(
            payload["artifact_version"],
            json!("workspace-agent-preflight-contract-v1")
        );
        assert_eq!(
            payload["preflight_contract_sha256_scope"],
            json!("preflight_contract object only")
        );
        assert_eq!(payload["preflight_contract_sha256"], json!(sha256));
        assert_eq!(
            payload["preflight_contract"]["contract_version"],
            json!("agent-preflight-contract-v1")
        );
        assert_eq!(
            payload["preflight_contract"]["status_sources"]["status_snapshot_path"],
            json!("docs/IMPLEMENTATION_STATUS.md")
        );
        assert_eq!(
            payload["preflight_contract"]["refresh_commands"]["shell_command"],
            json!("./scripts/agent_preflight.sh")
        );
        assert_eq!(
            payload["preflight_contract"]["runtime_state_artifact"]["workspace_runtime_state_relative_path"],
            json!(".amai/onboarding/project-agent-preflight-state.json")
        );
        assert_eq!(
            payload["preflight_contract"]["required_documents"][0]["path"],
            json!("AGENTS.md")
        );
    }

    #[test]
    fn renders_agent_preflight_state_artifact_with_live_stage_snapshot() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
        let text =
            render_agent_preflight_state_artifact(repo, repo).expect("agent preflight state");
        let payload: Value = serde_json::from_str(&text).expect("agent preflight state json");
        assert_eq!(
            payload["artifact_version"],
            json!("workspace-agent-preflight-state-v1")
        );
        assert_eq!(
            payload["agent_preflight_summary"]["status_snapshot_path"],
            json!("docs/IMPLEMENTATION_STATUS.md")
        );
        assert!(
            payload["agent_preflight_summary"]["stage_checklist"]
                .as_array()
                .is_some_and(|items| !items.is_empty())
        );
        let stage_progress_state = payload["agent_preflight_summary"]["stage_progress_state"]
            .as_str()
            .expect("stage_progress_state must be surfaced");
        assert!(
            matches!(
                stage_progress_state,
                "next_stage_required" | "all_stages_closed"
            ),
            "unexpected stage_progress_state: {stage_progress_state}"
        );
        if stage_progress_state == "next_stage_required" {
            assert!(
                payload["agent_preflight_summary"]["next_required_stage"]["label"]
                    .as_str()
                    .is_some_and(|label| !label.is_empty())
            );
            assert!(
                payload["agent_preflight_summary"]["next_stage_ready_mechanisms"]
                    .as_array()
                    .is_some_and(|items| !items.is_empty())
            );
        } else {
            assert!(payload["agent_preflight_summary"]["next_required_stage"].is_null());
            assert!(
                payload["agent_preflight_summary"]["next_stage_ready_mechanisms"]
                    .as_array()
                    .is_some_and(|items| items.is_empty())
            );
            assert!(
                payload["agent_preflight_summary"]["stage_checklist"]
                    .as_array()
                    .is_some_and(|items| items
                        .iter()
                        .all(|item| item["checked"].as_bool() == Some(true)))
            );
        }
        assert!(
            payload["source_documents"]
                .as_array()
                .is_some_and(|items| items.iter().any(|item| item["path"] == json!("AGENTS.md")))
        );
    }

    #[test]
    fn renders_external_workspace_startup_artifacts_with_helper_repo_paths() {
        let helper_repo = Path::new("/tmp/amai-helper");
        let workspace = Path::new("/tmp/bug-bounty");
        let (contract_text, contract_sha) =
            render_startup_contract_artifact(workspace, helper_repo)
                .expect("external startup contract must render");
        let contract: Value =
            serde_json::from_str(&contract_text).expect("external startup contract json");
        assert_eq!(contract["repo_root"], json!("/tmp/bug-bounty"));
        assert_eq!(contract["startup_contract_sha256"], json!(contract_sha));
        assert_eq!(
            contract["startup_contract"]["tool_runtime_reconcile"]["local_cli"]["shell_command"],
            json!("/tmp/amai-helper/scripts/continuity_startup.sh")
        );
        assert_eq!(
            contract["startup_contract"]["runtime_state_artifact"]["inspection_fallback_cli"]["shell_command"],
            json!("/tmp/amai-helper/scripts/continuity_startup_state.sh")
        );
        assert_eq!(
            contract["startup_contract"]["live_client_budget_enforcement"]["guard_shell_command"],
            json!("/tmp/amai-helper/scripts/client_budget_gate.sh")
        );
        assert_eq!(
            contract["startup_contract"]["live_client_budget_enforcement"]["compact_diagnostics_shell_command"],
            json!("/tmp/amai-helper/scripts/client_budget_root_cause.sh")
        );
        assert_eq!(
            contract["startup_contract"]["live_client_budget_enforcement"]["target_control"]["shell_command"],
            json!("/tmp/amai-helper/scripts/continuity_client_budget_target.sh")
        );
        assert_eq!(
            contract["startup_contract"]["live_client_budget_enforcement"]["compact_chat_control"]
                ["shell_command"],
            json!("/tmp/amai-helper/scripts/continuity_compact_chat.sh")
        );

        let agent_text = render_startup_agent_contract_artifact(workspace, helper_repo)
            .expect("external startup agent contract must render");
        let agent: Value =
            serde_json::from_str(&agent_text).expect("external startup agent contract json");
        assert_eq!(
            agent["compact_runtime_pointers"]["guard_shell_command"],
            json!("/tmp/amai-helper/scripts/client_budget_gate.sh")
        );
        assert_eq!(
            agent["compact_runtime_pointers"]["reconcile_local_cli_shell_command"],
            json!("/tmp/amai-helper/scripts/continuity_startup.sh")
        );

        let instructions = render_startup_instructions(
            workspace,
            helper_repo,
            "Codex",
            "codex",
            "codex_agents_snippet",
        )
        .expect("external startup instructions must render");
        assert!(instructions.contains("Workspace = `/tmp/bug-bounty`"));
        assert!(instructions.contains("/tmp/amai-helper/scripts/client_budget_gate.sh"));
        assert!(instructions.contains("/tmp/amai-helper/scripts/continuity_startup.sh"));
        assert!(instructions.contains("/tmp/amai-helper/scripts/reconnect_local.sh --client codex --cwd /tmp/amai-helper --workspace-root /tmp/bug-bounty --yes"));
        assert!(instructions.contains("/tmp/amai-helper/scripts/amai_exec.sh bootstrap reconnect --client codex --cwd /tmp/amai-helper --workspace-root /tmp/bug-bounty --yes"));
    }

    #[test]
    fn startup_artifact_audit_reports_ok_for_matching_install_state() {
        let unique = format!(
            "amai-startup-artifact-audit-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("epoch")
                .as_nanos()
        );
        let repo = std::env::temp_dir().join(unique);
        fs::create_dir_all(&repo).expect("temp repo");

        let startup_contract_path = startup_contract_artifact_path(&repo);
        if let Some(parent) = startup_contract_path.parent() {
            fs::create_dir_all(parent).expect("startup contract dir");
        }
        let (contract_text, contract_sha) =
            render_startup_contract_artifact(&repo, &repo).expect("startup contract");
        fs::write(&startup_contract_path, contract_text).expect("write startup contract");

        let startup_instruction_path =
            repo.join(".github/instructions/amai-continuity-startup.instructions.md");
        if let Some(parent) = startup_instruction_path.parent() {
            fs::create_dir_all(parent).expect("startup instruction dir");
        }
        let startup_instructions = render_startup_instructions(
            &repo,
            &repo,
            "VS Code",
            "vscode",
            "vscode_instructions_md",
        )
        .expect("startup instructions");
        fs::write(&startup_instruction_path, startup_instructions)
            .expect("write startup instructions");

        save_install_state(
            &repo,
            &InstallState {
                package_version: "0.1.0".to_string(),
                repo_revision: "test".to_string(),
                client_key: "vscode".to_string(),
                client_config: repo.join(".vscode/mcp.json").display().to_string(),
                stack_profile: "default".to_string(),
                installed_at_epoch_seconds: 1,
                memory_bridge_path: None,
                memory_bridge_backup_path: None,
                startup_instruction_path: Some(startup_instruction_path.display().to_string()),
                startup_instruction_status: Some(
                    "managed_workspace_instruction_installed".to_string(),
                ),
                startup_contract_path: Some(startup_contract_path.display().to_string()),
                startup_contract_status: Some(
                    "workspace_startup_contract_materialized".to_string(),
                ),
                startup_contract_sha256: Some(contract_sha),
                client_runtime_path: None,
                client_runtime_status: None,
                agent_preflight_contract_path: None,
                agent_preflight_agent_contract_path: None,
                agent_preflight_state_path: None,
                agent_preflight_contract_status: None,
                agent_preflight_contract_sha256: None,
            },
        )
        .expect("save install state");

        let audit = inspect_startup_artifacts(&repo)
            .expect("startup artifact audit")
            .expect("startup artifact audit payload");
        assert_eq!(audit.status, "ok");
        assert_eq!(audit.startup_instruction_contains_expected_sha, Some(true));
        assert_eq!(
            audit.startup_instruction_contains_required_before_tool_call,
            Some(true)
        );
        assert_eq!(
            audit.startup_instruction_contains_missing_fail_closed,
            Some(true)
        );
        assert_eq!(
            audit.startup_instruction_contains_sha_mismatch_fail_closed,
            Some(true)
        );
        assert_eq!(
            audit.startup_instruction_contains_startup_next_action,
            Some(true)
        );
        assert_eq!(
            audit.startup_instruction_contains_required_return_task,
            Some(true)
        );
        assert_eq!(
            audit.startup_instruction_contains_resume_required_action_kind,
            Some(true)
        );
        assert_eq!(
            audit.startup_instruction_contains_execctl_resume_contract_summary,
            Some(true)
        );
        assert_eq!(
            audit.startup_instruction_contains_execctl_resume_obligation,
            Some(true)
        );
        assert_eq!(
            audit.startup_instruction_contains_execctl_active_lease_summary,
            Some(true)
        );
        assert_eq!(
            audit.startup_instruction_contains_lease_owner_state,
            Some(true)
        );
        assert_eq!(
            audit.startup_instruction_contains_previous_session_owner_value,
            Some(true)
        );
        assert_eq!(
            audit.startup_instruction_contains_previous_session_owner_follow,
            Some(true)
        );
        assert_eq!(
            audit.startup_instruction_contains_no_silent_drop,
            Some(true)
        );
        assert_eq!(
            audit.startup_instruction_contains_runtime_state_artifact,
            Some(true)
        );
        assert_eq!(
            audit.startup_instruction_contains_runtime_state_artifact_version,
            Some(true)
        );
        assert_eq!(
            audit.startup_instruction_contains_runtime_state_written_by_tool,
            Some(true)
        );
        assert_eq!(
            audit.startup_instruction_contains_runtime_state_source_summary_field,
            Some(true)
        );
        assert_eq!(
            audit.startup_instruction_contains_project_task_tree,
            Some(true)
        );
        assert_eq!(
            audit.startup_instruction_contains_project_task_tree_summary,
            Some(true)
        );
        assert_eq!(
            audit.startup_instruction_contains_project_task_ledger,
            Some(true)
        );
        assert_eq!(
            audit.startup_instruction_contains_project_task_ledger_summary,
            Some(true)
        );
        assert_eq!(
            audit.startup_instruction_contains_startup_execution_gate,
            Some(true)
        );
        assert_eq!(
            audit.startup_instruction_contains_startup_state_fallback_cli,
            Some(true)
        );
        assert_eq!(
            audit.startup_instruction_contains_gate_field_enforcement,
            Some(true)
        );
        assert_eq!(
            audit.startup_instruction_contains_gate_semantics_consistent,
            Some(true)
        );
        assert_eq!(
            audit.startup_contract_sha_matches_current_contract,
            Some(true)
        );
        assert_eq!(audit.install_state_sha_matches_current_contract, Some(true));
        assert_eq!(audit.startup_contract_enforces_fail_closed, Some(true));
        assert_eq!(
            audit.startup_contract_contains_startup_execution_gate_field,
            Some(true)
        );
        assert_eq!(
            audit.startup_contract_contains_startup_next_action_field,
            Some(true)
        );
        assert_eq!(
            audit.startup_contract_contains_required_return_task_field,
            Some(true)
        );
        assert_eq!(
            audit.startup_contract_contains_resume_required_action_kind,
            Some(true)
        );
        assert_eq!(
            audit.startup_contract_contains_active_lease_owner_state_field,
            Some(true)
        );
        assert_eq!(
            audit.startup_contract_contains_previous_session_owner_value,
            Some(true)
        );
        assert_eq!(
            audit.startup_contract_contains_previous_session_owner_follow,
            Some(true)
        );
        assert_eq!(audit.startup_contract_contains_no_silent_drop, Some(true));
        assert_eq!(
            audit.startup_contract_contains_runtime_state_artifact,
            Some(true)
        );
        assert_eq!(
            audit.startup_contract_contains_runtime_state_artifact_version,
            Some(true)
        );
        assert_eq!(
            audit.startup_contract_contains_startup_execution_gate,
            Some(true)
        );
        assert_eq!(
            audit.startup_contract_contains_startup_state_fallback_cli,
            Some(true)
        );
        assert_eq!(
            audit.startup_contract_contains_gate_semantics_consistent_field,
            Some(true)
        );
        assert_eq!(
            audit.startup_contract_requires_gate_semantics_consistent_true,
            Some(true)
        );
        assert_eq!(
            audit.startup_contract_contains_gate_field_enforcement,
            Some(true)
        );
        assert_eq!(
            audit.startup_contract_enforces_gate_field_semantics,
            Some(true)
        );

        fs::remove_dir_all(&repo).expect("cleanup temp repo");
    }

    #[test]
    fn hermes_compact_startup_instructions_render_as_contract_pointer() {
        let repo = Path::new("/tmp/amai-hermes");
        let instructions =
            render_startup_instructions(repo, repo, "Hermes", "hermes", "hermes_compact_markdown")
                .expect("hermes startup instructions must render");
        assert!(instructions.contains("compact contract-pointer"));
        assert!(instructions.contains("startup_contract_sha256 = \""));
        assert!(instructions.contains("./scripts/client_budget_gate.sh --enforce-reply-gate"));
        assert!(instructions.contains("./scripts/reconnect_local.sh --client hermes"));
        assert!(instructions.contains(".amai/continuity/project-chat-startup-state.json"));
        assert!(!instructions.contains("Exact operator-switch для target режима"));
    }

    #[test]
    fn startup_artifact_audit_reports_ok_for_external_workspace_install_state() {
        let unique = format!(
            "amai-external-startup-artifact-audit-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("epoch")
                .as_nanos()
        );
        let helper_repo = std::env::temp_dir().join(format!("{unique}-helper"));
        let workspace = std::env::temp_dir().join(format!("{unique}-workspace"));
        fs::create_dir_all(&helper_repo).expect("temp helper repo");
        fs::create_dir_all(&workspace).expect("temp workspace");

        let startup_contract_path = startup_contract_artifact_path(&workspace);
        if let Some(parent) = startup_contract_path.parent() {
            fs::create_dir_all(parent).expect("startup contract dir");
        }
        let (contract_text, contract_sha) =
            render_startup_contract_artifact(&workspace, &helper_repo).expect("startup contract");
        fs::write(&startup_contract_path, contract_text).expect("write startup contract");

        let startup_instruction_path = workspace.join("AGENTS.md");
        let startup_instructions = render_startup_instructions(
            &workspace,
            &helper_repo,
            "Codex",
            "codex",
            "codex_agents_snippet",
        )
        .expect("startup instructions");
        fs::write(&startup_instruction_path, startup_instructions)
            .expect("write startup instructions");

        save_install_state(
            &helper_repo,
            &InstallState {
                package_version: "0.1.0".to_string(),
                repo_revision: "test".to_string(),
                client_key: "codex".to_string(),
                client_config: helper_repo.join(".codex/config.toml").display().to_string(),
                stack_profile: "default".to_string(),
                installed_at_epoch_seconds: 1,
                memory_bridge_path: None,
                memory_bridge_backup_path: None,
                startup_instruction_path: Some(startup_instruction_path.display().to_string()),
                startup_instruction_status: Some(
                    "managed_append_instruction_installed".to_string(),
                ),
                startup_contract_path: Some(startup_contract_path.display().to_string()),
                startup_contract_status: Some(
                    "workspace_startup_contract_materialized".to_string(),
                ),
                startup_contract_sha256: Some(contract_sha),
                client_runtime_path: None,
                client_runtime_status: None,
                agent_preflight_contract_path: None,
                agent_preflight_agent_contract_path: None,
                agent_preflight_state_path: None,
                agent_preflight_contract_status: None,
                agent_preflight_contract_sha256: None,
            },
        )
        .expect("save install state");

        let audit = inspect_startup_artifacts(&helper_repo)
            .expect("startup artifact audit")
            .expect("startup artifact audit payload");
        assert_eq!(audit.status, "ok");
        assert_eq!(audit.startup_instruction_contains_expected_sha, Some(true));
        assert_eq!(audit.install_state_sha_matches_current_contract, Some(true));
        assert_eq!(
            audit.startup_contract_sha_matches_current_contract,
            Some(true)
        );

        fs::remove_dir_all(&helper_repo).expect("cleanup helper repo");
        fs::remove_dir_all(&workspace).expect("cleanup workspace");
    }

    #[test]
    fn startup_artifact_audit_accepts_hermes_compact_contract_pointer() {
        let unique = format!(
            "amai-hermes-compact-audit-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("epoch")
                .as_nanos()
        );
        let helper_repo = std::env::temp_dir().join(format!("{unique}-helper"));
        let workspace = std::env::temp_dir().join(format!("{unique}-workspace"));
        fs::create_dir_all(&helper_repo).expect("temp helper repo");
        fs::create_dir_all(&workspace).expect("temp workspace");

        let startup_contract_path = startup_contract_artifact_path(&workspace);
        if let Some(parent) = startup_contract_path.parent() {
            fs::create_dir_all(parent).expect("startup contract dir");
        }
        let (contract_text, contract_sha) =
            render_startup_contract_artifact(&workspace, &helper_repo).expect("startup contract");
        fs::write(&startup_contract_path, contract_text).expect("write startup contract");

        let startup_instruction_path = workspace.join(".hermes.md");
        let startup_instructions = render_startup_instructions(
            &workspace,
            &helper_repo,
            "Hermes",
            "hermes",
            "hermes_compact_markdown",
        )
        .expect("hermes startup instructions");
        fs::write(&startup_instruction_path, startup_instructions)
            .expect("write startup instructions");

        save_install_state(
            &helper_repo,
            &InstallState {
                package_version: "0.1.0".to_string(),
                repo_revision: "test".to_string(),
                client_key: "hermes".to_string(),
                client_config: helper_repo
                    .join(".hermes/config.yaml")
                    .display()
                    .to_string(),
                stack_profile: "default".to_string(),
                installed_at_epoch_seconds: 1,
                memory_bridge_path: None,
                memory_bridge_backup_path: None,
                startup_instruction_path: Some(startup_instruction_path.display().to_string()),
                startup_instruction_status: Some(
                    "managed_workspace_instruction_installed".to_string(),
                ),
                startup_contract_path: Some(startup_contract_path.display().to_string()),
                startup_contract_status: Some(
                    "workspace_startup_contract_materialized".to_string(),
                ),
                startup_contract_sha256: Some(contract_sha),
                client_runtime_path: None,
                client_runtime_status: None,
                agent_preflight_contract_path: None,
                agent_preflight_agent_contract_path: None,
                agent_preflight_state_path: None,
                agent_preflight_contract_status: None,
                agent_preflight_contract_sha256: None,
            },
        )
        .expect("save install state");

        let audit = inspect_startup_artifacts(&helper_repo)
            .expect("startup artifact audit")
            .expect("startup artifact audit payload");
        assert_eq!(audit.status, "ok");
        assert_eq!(audit.startup_instruction_contains_expected_sha, Some(true));
        assert_eq!(
            audit.startup_instruction_contains_execctl_active_lease_summary,
            Some(false)
        );
        assert_eq!(
            audit.startup_instruction_contains_runtime_state_artifact_version,
            Some(false)
        );
        assert_eq!(audit.install_state_sha_matches_current_contract, Some(true));
        assert_eq!(
            audit.startup_contract_sha_matches_current_contract,
            Some(true)
        );

        fs::remove_dir_all(&helper_repo).expect("cleanup helper repo");
        fs::remove_dir_all(&workspace).expect("cleanup workspace");
    }

    #[tokio::test]
    async fn remove_vscode_bridge_install_removes_bundle_and_registry_entries() {
        let unique = format!(
            "amai-vscode-bridge-remove-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("epoch")
                .as_nanos()
        );
        let home = std::env::temp_dir().join(unique);
        let vscode_root = home.join(".vscode/extensions");
        let vscodium_root = home.join(".vscode-oss/extensions");
        fs::create_dir_all(&vscode_root).expect("vscode extensions root");
        fs::create_dir_all(&vscodium_root).expect("vscodium extensions root");
        let current_bundle = vscode_root.join("amai.amai-vscode-bridge-0.0.3");
        let legacy_bundle = vscodium_root.join("art-local.amai-vscode-bridge-0.0.2");
        fs::create_dir_all(&current_bundle).expect("current bundle");
        fs::create_dir_all(&legacy_bundle).expect("legacy bundle");
        fs::write(current_bundle.join("package.json"), "{}").expect("current package");
        fs::write(legacy_bundle.join("package.json"), "{}").expect("legacy package");
        fs::write(
            vscode_root.join("extensions.json"),
            r#"[
  {"identifier":{"id":"amai.amai-vscode-bridge"},"version":"0.0.3"},
  {"identifier":{"id":"other.vscode"},"version":"1.0.0"}
]"#,
        )
        .expect("vscode registry");
        fs::write(
            vscodium_root.join("extensions.json"),
            r#"[
  {"identifier":{"id":"art-local.amai-vscode-bridge"},"version":"0.0.2"},
  {"identifier":{"id":"other.vscodium"},"version":"1.0.0"}
]"#,
        )
        .expect("registry");

        let previous_home = std::env::var_os("HOME");
        unsafe { std::env::set_var("HOME", &home) };
        let removed = super::remove_vscode_bridge_install()
            .await
            .expect("remove vscode bridge install");

        assert!(removed);
        assert!(!current_bundle.exists());
        assert!(!legacy_bundle.exists());
        let vscode_registry: Value = serde_json::from_str(
            &fs::read_to_string(vscode_root.join("extensions.json")).expect("read vscode registry"),
        )
        .expect("parse vscode registry");
        let vscode_ids: Vec<String> = vscode_registry
            .as_array()
            .expect("vscode registry array")
            .iter()
            .filter_map(|entry| {
                entry["identifier"]["id"]
                    .as_str()
                    .map(|value| value.to_string())
            })
            .collect();
        assert_eq!(vscode_ids, vec!["other.vscode".to_string()]);
        let vscodium_registry: Value = serde_json::from_str(
            &fs::read_to_string(vscodium_root.join("extensions.json"))
                .expect("read vscodium registry"),
        )
        .expect("parse vscodium registry");
        let vscodium_ids: Vec<String> = vscodium_registry
            .as_array()
            .expect("vscodium registry array")
            .iter()
            .filter_map(|entry| {
                entry["identifier"]["id"]
                    .as_str()
                    .map(|value| value.to_string())
            })
            .collect();
        assert_eq!(vscodium_ids, vec!["other.vscodium".to_string()]);

        if let Some(previous_home) = previous_home {
            unsafe { std::env::set_var("HOME", previous_home) };
        } else {
            unsafe { std::env::remove_var("HOME") };
        }
        fs::remove_dir_all(&home).expect("cleanup temp home");
    }

    #[test]
    fn managed_startup_block_appends_to_existing_rules_file() {
        let repo = Path::new("/tmp/amai");
        let block =
            render_startup_instructions(repo, repo, "Codex", "codex", "codex_agents_snippet")
                .expect("codex startup instructions must render");
        let existing = "# Existing project rules\n\n- keep this content\n";
        let merged = merge_managed_startup_block(existing, &block).expect("managed merge");
        assert!(merged.contains("# Existing project rules"));
        assert!(merged.contains("AMAI MANAGED STARTUP INSTRUCTIONS v1"));
        assert!(merged.contains("/AMAI MANAGED STARTUP INSTRUCTIONS v1"));
        assert!(merged.contains("project `AGENTS.md`"));
    }

    #[test]
    fn strip_managed_startup_block_removes_only_embedded_block() {
        let repo = Path::new("/tmp/amai");
        let block =
            render_startup_instructions(repo, repo, "Codex", "codex", "codex_agents_snippet")
                .expect("codex startup instructions must render");
        let existing = format!("# Existing project rules\n\n{block}\n## Keep me too\n");
        let stripped = strip_managed_startup_block(&existing)
            .expect("managed strip should succeed")
            .expect("managed block should be found");
        assert!(stripped.contains("# Existing project rules"));
        assert!(stripped.contains("## Keep me too"));
        assert!(!stripped.contains("AMAI MANAGED STARTUP INSTRUCTIONS v1"));
        assert!(!stripped.contains("amai_continuity_startup"));
    }

    #[test]
    fn working_state_reason_summary_uses_summary_field_then_trace_fallback() {
        let snapshot = json!({
            "latest_working_state_restore": {
                "working_state_restore": {
                    "included_reasons_summary": "точные совпадения (1) — Нашлись точные совпадения.",
                    "latest_decision_trace": {
                        "included": [{
                            "strategy": "exact_documents",
                            "count": 1,
                            "reason": "fallback should not win"
                        }],
                        "not_included": [{
                            "strategy": "semantic_chunks",
                            "reason": "Semantic layer abstained."
                        }]
                    }
                }
            }
        });
        assert_eq!(
            working_state_reason_summary(&snapshot, "included_reasons_summary", "included")
                .as_deref(),
            Some("точные совпадения (1) — Нашлись точные совпадения.")
        );
        assert_eq!(
            working_state_reason_summary(&snapshot, "excluded_reasons_summary", "not_included")
                .as_deref(),
            Some("смысловые фрагменты — Semantic layer abstained.")
        );
    }
}
