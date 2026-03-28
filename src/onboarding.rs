use crate::cli::{BootstrapDisconnectArgs, BootstrapOnboardingArgs, McpConfigArgs};
use crate::config;
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
    let output = resolve_output_path(&repo_root, &target, args.output.as_ref())?;
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

        check_dependency("docker", &["--version"]).await?;
        check_dependency("cargo", &["--version"]).await?;

        if !args.skip_stack {
            let mut bootstrap_stack = script_command(
                &repo_root,
                "scripts/bootstrap_stack.sh",
                ["--stack-profile", args.stack_profile.as_str()],
            );
            bootstrap_stack.env("AMAI_SKIP_STACK_PREFLIGHT", "1");
            run_command("bootstrap stack", bootstrap_stack).await?;
        }

        if !args.skip_release_build {
            run_command(
                "cargo build --release",
                command_in(&repo_root, "cargo", ["build", "--release"]),
            )
            .await?;
        }

        local_memory_bridge_summary = Some(install_memory_bridge(&repo_root)?);

        if let Ok(cfg) = config::AppConfig::from_env() {
            local_metrics_summary = collect_install_metrics_summary(&cfg).await.ok();
        }
    }
    let backup = maybe_backup_user_global(&output, &target.install_scope)?;
    mcp::write_client_config(&config_args)?;
    let startup_contract_summary = install_startup_contract_artifact(&repo_root)?;
    let startup_instructions_summary =
        install_startup_instructions(&repo_root, &client_resolution)?;
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
    let expected_contract_sha = startup_contract_sha256(&mcp::project_chat_startup_contract())?;
    let startup_instruction_path = state.startup_instruction_path.as_ref().map(PathBuf::from);
    let startup_instruction_exists = startup_instruction_path
        .as_ref()
        .map(|path| path.is_file())
        .unwrap_or(false);
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
        let path = startup_instruction_path
            .as_ref()
            .expect("startup instruction path must exist when marked present");
        let content = fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
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
            Some(content.contains("continuity startup-state --repo-root")),
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

    let startup_contract_path = state.startup_contract_path.as_ref().map(PathBuf::from);
    let startup_contract_exists = startup_contract_path
        .as_ref()
        .map(|path| path.is_file())
        .unwrap_or(false);
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
    let status = if !startup_instruction_exists {
        "missing_startup_instruction".to_string()
    } else if !startup_contract_exists {
        "missing_startup_contract".to_string()
    } else if startup_instruction_contains_expected_sha != Some(true)
        || startup_instruction_contains_required_before_tool_call != Some(true)
        || startup_instruction_contains_missing_fail_closed != Some(true)
        || startup_instruction_contains_sha_mismatch_fail_closed != Some(true)
        || startup_instruction_contains_startup_next_action != Some(true)
        || startup_instruction_contains_required_return_task != Some(true)
        || startup_instruction_contains_resume_required_action_kind != Some(true)
        || startup_instruction_contains_execctl_resume_contract_summary != Some(true)
        || startup_instruction_contains_execctl_resume_obligation != Some(true)
        || startup_instruction_contains_execctl_active_lease_summary != Some(true)
        || startup_instruction_contains_lease_owner_state != Some(true)
        || startup_instruction_contains_previous_session_owner_value != Some(true)
        || startup_instruction_contains_previous_session_owner_follow != Some(true)
        || startup_instruction_contains_no_silent_drop != Some(true)
        || startup_instruction_contains_runtime_state_artifact != Some(true)
        || startup_instruction_contains_runtime_state_artifact_version != Some(true)
        || startup_instruction_contains_runtime_state_written_by_tool != Some(true)
        || startup_instruction_contains_runtime_state_source_summary_field != Some(true)
        || startup_instruction_contains_project_task_tree != Some(true)
        || startup_instruction_contains_project_task_tree_summary != Some(true)
        || startup_instruction_contains_project_task_ledger != Some(true)
        || startup_instruction_contains_project_task_ledger_summary != Some(true)
        || startup_instruction_contains_startup_execution_gate != Some(true)
        || startup_instruction_contains_startup_state_fallback_cli != Some(true)
        || startup_instruction_contains_gate_field_enforcement != Some(true)
        || startup_instruction_contains_gate_semantics_consistent != Some(true)
    {
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
    let repo_root = discover_repo_root(args.cwd.as_deref())?;
    let install_state_before = load_install_state(&repo_root)?;
    let client_resolution = resolve_client_target(&repo_root, &args.client, false)?;
    let target = client_resolution.target.clone();
    let output = resolve_output_path(&repo_root, &target, args.output.as_ref())?;
    let backup = maybe_backup_user_global(&output, &target.install_scope)?;
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

    println!("disconnect completed");
    println!("client: {}", client_resolution.client_key);
    println!("client_display_name: {}", target.display_name);
    println!(
        "client_resolution_mode: {}",
        if client_resolution.auto_selected {
            "auto_detected"
        } else {
            "explicit"
        }
    );
    println!("client_detection_reason: {}", client_resolution.reason);
    println!("client_config: {}", output.display());
    println!("server_removed: {}", result.removed);
    println!("file_purged: {}", result.purged_file);
    if let Some(summary) = startup_instructions_removed {
        println!("startup_instruction_removed: true");
        println!(
            "startup_instruction_path: {}",
            summary.output_path.display()
        );
        println!("startup_instruction_status: {}", summary.status);
    } else {
        println!("startup_instruction_removed: false");
    }
    if let Some(state) = &install_state_before {
        if let Some(startup_contract_path) = &state.startup_contract_path {
            println!("startup_contract_path: {}", startup_contract_path);
        }
        if let Some(startup_contract_status) = &state.startup_contract_status {
            println!("startup_contract_status: {}", startup_contract_status);
        }
        if let Some(startup_contract_sha256) = &state.startup_contract_sha256 {
            println!("startup_contract_sha256: {}", startup_contract_sha256);
        }
    }
    if let Some(state) = &install_state_before {
        if let Some(memory_bridge_path) = &state.memory_bridge_path {
            println!("memory_bridge: {}", memory_bridge_path);
        }
        if let Some(memory_bridge_backup_path) = &state.memory_bridge_backup_path {
            println!("memory_bridge_backup: {}", memory_bridge_backup_path);
        }
    }
    if let Some(summary) =
        restore_memory_bridge(repo_root.as_path(), install_state_before.as_ref())?
    {
        println!("memory_bridge_restore: {}", summary);
    }
    if let Some(backup) = backup {
        println!("backup_file: {}", backup.display());
    }
    println!("next_step_1: reload the client window or restart the client");
    println!("next_step_2: verify that Amai is no longer listed as an MCP server");
    Ok(())
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

async fn check_dependency(program: &str, args: &[&str]) -> Result<()> {
    let status = Command::new(program)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .with_context(|| format!("failed to start dependency check for {program}"))?;
    if !status.success() {
        bail!("{program} is required for onboarding but is not available");
    }
    Ok(())
}

fn command_in<const N: usize>(repo_root: &Path, program: &str, args: [&str; N]) -> Command {
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
struct StartupContractInstallSummary {
    status: String,
    output_path: PathBuf,
    install_scope: String,
    reason: String,
    sha256: String,
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
    repo_root: &Path,
    client_resolution: &ClientResolution,
) -> Result<Option<StartupInstructionsInstallSummary>> {
    let Some(startup) = &client_resolution.target.startup_instructions else {
        return Ok(None);
    };
    let home = dirs::home_dir().ok_or_else(|| anyhow!("failed to resolve user home directory"))?;
    let output_path = expand_target_template(&startup.default_output, repo_root, &home);
    let content = render_startup_instructions(
        repo_root,
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
                    let fallback = repo_root.join("tmp/onboarding").join(format!(
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

fn install_startup_contract_artifact(
    repo_root: &Path,
) -> Result<Option<StartupContractInstallSummary>> {
    let output_path = startup_contract_artifact_path(repo_root);
    let agent_output_path = startup_agent_contract_artifact_path(repo_root);
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let (content, sha256) = render_startup_contract_artifact(repo_root)?;
    let agent_content = render_startup_agent_contract_artifact(repo_root)?;
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

fn render_startup_instructions(
    repo_root: &Path,
    client_display_name: &str,
    client_key: &str,
    format: &str,
) -> Result<String> {
    let body = render_startup_instruction_body(repo_root)?;
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
        "generic_markdown" => Ok(format!(
            "{STARTUP_INSTRUCTIONS_MARKER}\n# Amai continuity startup ({client_display_name})\n\n{body}\n{STARTUP_INSTRUCTIONS_END_MARKER}\n"
        )),
        other => Err(anyhow!(
            "unsupported startup instructions format for client {client_key}: {other}"
        )),
    }
}

fn render_startup_instruction_body(repo_root: &Path) -> Result<String> {
    let contract = mcp::project_chat_startup_contract();
    let contract_path = startup_contract_artifact_path(repo_root);
    let agent_contract_path = startup_agent_contract_artifact_path(repo_root);
    let startup_contract_sha256 = startup_contract_sha256(&contract)?;
    let tool = contract["tool"]
        .as_str()
        .ok_or_else(|| anyhow!("project_chat_startup contract is missing tool"))?;
    let namespace = contract["default_namespace"]
        .as_str()
        .ok_or_else(|| anyhow!("project_chat_startup contract is missing default_namespace"))?;
    let artifact_enforcement = &contract["artifact_enforcement"];
    let startup_contract_relative_path = artifact_enforcement["workspace_contract_relative_path"]
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
    let client_budget_prefer_compact_diagnostics =
        client_budget_enforcement["must_prefer_compact_diagnostics_over_full_snapshot"]
            .as_bool()
            .unwrap_or(false);
    let client_budget_must_check_before_each_reply =
        client_budget_enforcement["must_check_before_each_substantive_reply"]
            .as_bool()
            .unwrap_or(false);
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
                .collect::<Vec<_>>()
                .join(", ")
        })
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "новый чат нужен сейчас".to_string());
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
    let client_budget_blocking_reply_contract_field =
        client_budget_enforcement["blocking_reply_contract_field"]
            .as_str()
            .unwrap_or("blocking_reply_contract");
    let client_budget_blocking_reply_contract_version =
        client_budget_enforcement["blocking_reply_contract_version"]
            .as_str()
            .unwrap_or(working_state::CLIENT_BUDGET_BLOCKING_REPLY_CONTRACT_VERSION);
    let client_budget_blocking_reply_response_kind =
        client_budget_enforcement["blocking_reply_response_kind"]
            .as_str()
            .unwrap_or(working_state::CLIENT_BUDGET_BLOCKING_REPLY_RESPONSE_KIND);
    let client_budget_blocking_reply_max_sentences =
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
    let client_budget_blocking_reply_template =
        client_budget_enforcement["blocking_reply_template"]
            .as_str()
            .unwrap_or(working_state::CLIENT_BUDGET_BLOCKING_REPLY_TEMPLATE);
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
    let client_budget_blocking_reply_must_avoid_substantive_work_text =
        if client_budget_blocking_reply_must_avoid_substantive_work {
            "true"
        } else {
            "false"
        };
    let client_budget_blocking_reply_must_use_action_bundle_operator_flow_text =
        if client_budget_blocking_reply_must_use_action_bundle_operator_flow {
            "true"
        } else {
            "false"
        };
    let client_budget_full_scale_truth_required_text = if client_budget_full_scale_truth_required {
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
    let client_budget_prefer_compact_diagnostics_text = if client_budget_prefer_compact_diagnostics
    {
        "true"
    } else {
        "false"
    };
    let repo_root_display = repo_root.display().to_string();
    let contract_path_display = contract_path.display().to_string();
    let agent_contract_path_display = agent_contract_path.display().to_string();
    let startup_agent_contract_relative_path =
        ".amai/onboarding/project-chat-startup-agent-contract.json";

    Ok(format!(
        "Перед первым содержательным ответом в новом или resumed чате, а затем перед каждым следующим содержательным ответом, пока этот чат жив:\n1. Считай текущий workspace проектом с repo root `{repo_root_display}`. Сначала прочитай compact agent contract `{agent_contract_path_display}` (relative path `{startup_agent_contract_relative_path}`), а полный machine-readable startup contract `{contract_path_display}` (relative path `{startup_contract_relative_path}`) используй как pinned source-of-truth для sha/fail-closed law. До MCP tool call проверь literal `{startup_contract_sha256_field} = \"{startup_contract_sha256}\"`, `workspace_contract_required_before_tool_call = {startup_contract_required_before_tool_call_text}`, `missing_or_unreadable_fail_closed = {startup_contract_missing_or_unreadable_fail_closed_text}`, `sha256_mismatch_fail_closed = {startup_contract_sha256_mismatch_fail_closed_text}`.\n2. Затем вызови MCP tool `{tool}` с `repo_root = \"{repo_root_display}\"` и `namespace = \"{namespace}\"`; `project` передавай только при exact binding по repo_root. Не переходи к `amai_context_pack` и другим новым действиям, пока не получен `continuity_startup_summary`.\n3. После startup прочитай runtime artifact `{runtime_state_relative_path}`; его пишет `{runtime_state_written_by_tool}`, он обязан нести `{runtime_state_source_summary_field}`, а `workspace_runtime_state_artifact_version` должен быть `{runtime_state_artifact_version}`. Если direct file-read неудобен, используй `cargo run -- {startup_state_fallback_cli} --repo-root \"{repo_root_display}\" --json`.\n4. В runtime artifact используй как pinned pointers: `{startup_execution_gate_field}`, `{resume_state_field}`, `{resume_contract_field}`, `{resume_obligation_field}`, `{startup_next_action_field}`, `{active_lease_field}`. Все обязательные restore fields бери из массива `required_summary_fields`, а все обязательные workline obligations бери из массива `restored_obligations`; не пересказывай и не подменяй эти списки вручную.\n5. Fail-closed, если `{gate_semantics_consistent_field} != true` (`gate_semantics_consistent_true_required = {gate_semantics_consistent_true_required_text}`), `{startup_execution_gate_field}.{gate_must_follow_field} != true`, `{startup_execution_gate_field}.{gate_unrelated_work_allowed_field} != false`, `{startup_execution_gate_field}.{gate_prompt_read_field} != true` или `{startup_execution_gate_field}.{gate_no_silent_drop_field} != true`.\n6. Resume law: если `{startup_execution_gate_field}.{gate_required_action_kind_field} == \"{required_action_kind}\"`, `{startup_next_action_field}.action_kind == \"{required_action_kind}\"` (`must_resume_required_return_task_before_unrelated_work = {must_resume_before_unrelated_text}`) или `{active_lease_field}.{active_lease_owner_state_field} == \"{previous_session_owner_value}\"` (`previous_session_owner_must_follow_startup_next_action = {previous_session_owner_must_follow_startup_next_action_text}`), follow startup_next_action first. Silent drop запрещён: `no_silent_drop = {no_silent_drop_text}`. Здесь же смотри `execctl_active_lease_summary`, `required_return_task`, `project_task_tree`, `project_task_tree_summary`, `project_task_ledger`, `project_task_ledger_summary`.\n7. Перед каждым содержательным ответом обновляй guard `cargo run -- {client_budget_guard_command}` и работай только по `{client_budget_guard_summary_field}.{client_budget_reply_execution_gate_field}`. `must_check_before_each_substantive_reply = {client_budget_must_check_before_each_reply_text}`, stale старше `{client_budget_max_guard_age_seconds_text}` секунд запрещён (`stale_guard_requires_refresh = {client_budget_stale_guard_requires_refresh_text}`), для hard gate automation используй `{client_budget_guard_enforcement_flag}` (`guard_enforcement_exit_on_blocking = {client_budget_guard_enforcement_exit_on_blocking_text}`).\n8. Для KPI/guard/exact-pair root-cause сначала используй `cargo run -- {client_budget_compact_diagnostics_command}`; `must_prefer_compact_diagnostics_over_full_snapshot = {client_budget_prefer_compact_diagnostics_text}` означает, что full `observe snapshot` без фильтра для этой задачи запрещён.\n9. Gate version pinned: `{client_budget_reply_execution_gate_version}`. Если `{client_budget_reply_budget_mode_field} == \"{client_budget_compact_reply_mode_value}\"`, substantive reply разрешён только по `{client_budget_reply_budget_contract_field}` с `contract_version = \"{client_budget_compact_reply_contract_version}\"`: direct answer first, no unrequested recap, no repeated known context, keep only changed facts when possible, prefer patch/result over narration when coding, preserve truthfulness/technical accuracy, disclose unknowns instead of guessing.\n10. Если `{client_budget_reply_execution_gate_field}.must_rotate_before_reply = true`, `{client_budget_rotate_now_field} = true` или `{client_budget_status_label_field}` равен одному из [{client_budget_rotate_status_labels}], сначала сохрани handoff (`save_handoff_before_rotate = {client_budget_save_handoff_before_rotate_text}`) и продолжай только в свежем чате через continuity startup (`fresh_chat_requires_continuity_startup = {client_budget_fresh_chat_requires_startup_text}`). В blocked path разрешён только `{client_budget_blocking_reply_contract_field}`: `contract_version = \"{client_budget_blocking_reply_contract_version}\"`, `response_kind = \"{client_budget_blocking_reply_response_kind}\"`, `max_sentences = {client_budget_blocking_reply_max_sentences}`, `must_avoid_substantive_work = {client_budget_blocking_reply_must_avoid_substantive_work_text}`, `must_use_action_bundle_operator_flow = {client_budget_blocking_reply_must_use_action_bundle_operator_flow_text}`. Pinned template: `{client_budget_blocking_reply_template}`.\n11. Не подменяй полную клиентскую шкалу внутренним Amai-slice: `full_scale_client_truth_required = {client_budget_full_scale_truth_required_text}`. Если startup вернул любой fail-closed scenario ({fail_closed}), прямо сообщай о блокере и не угадывай continuity."
    ))
}

fn render_startup_contract_artifact(repo_root: &Path) -> Result<(String, String)> {
    let contract = mcp::project_chat_startup_contract();
    let startup_contract_sha256 = startup_contract_sha256(&contract)?;
    let tool = contract["tool"]
        .as_str()
        .ok_or_else(|| anyhow!("project_chat_startup contract is missing tool"))?;
    let namespace = contract["default_namespace"]
        .as_str()
        .ok_or_else(|| anyhow!("project_chat_startup contract is missing default_namespace"))?;
    let payload = json!({
        "artifact_version": "workspace-startup-contract-v1",
        "contract_kind": "project_chat_startup",
        "repo_root": repo_root.display().to_string(),
        "default_namespace": namespace,
        "startup_contract_sha256": startup_contract_sha256,
        "startup_contract_sha256_scope": "startup_contract object only",
        "tool": tool,
        "recommended_startup_call": {
            "tool": tool,
            "arguments": {
                "repo_root": repo_root.display().to_string(),
                "namespace": namespace
            },
            "project_argument_rule": "pass project when already known, otherwise require exact binding by repo_root"
        },
        "startup_contract": contract
    });
    let content = serde_json::to_string_pretty(&payload)
        .context("failed to serialize startup contract artifact")?;
    Ok((content, startup_contract_sha256))
}

fn render_startup_agent_contract_artifact(repo_root: &Path) -> Result<String> {
    let contract = mcp::project_chat_startup_contract();
    let startup_contract_sha256 = startup_contract_sha256(&contract)?;
    let tool = contract["tool"]
        .as_str()
        .ok_or_else(|| anyhow!("project_chat_startup contract is missing tool"))?;
    let namespace = contract["default_namespace"]
        .as_str()
        .ok_or_else(|| anyhow!("project_chat_startup contract is missing default_namespace"))?;
    let payload = json!({
        "artifact_version": "workspace-startup-agent-contract-v1",
        "contract_kind": "project_chat_startup_agent_read",
        "repo_root": repo_root.display().to_string(),
        "default_namespace": namespace,
        "tool": tool,
        "full_startup_contract_relative_path": ".amai/onboarding/project-chat-startup-contract.json",
        "full_startup_contract_sha256": startup_contract_sha256,
        "recommended_startup_call": {
            "tool": tool,
            "arguments": {
                "repo_root": repo_root.display().to_string(),
                "namespace": namespace
            }
        },
        "artifact_enforcement": contract["artifact_enforcement"].clone(),
        "runtime_state_artifact": contract["runtime_state_artifact"].clone(),
        "required_summary_fields": contract["required_summary_fields"].clone(),
        "restored_obligations": contract["restored_obligations"].clone(),
        "resume_enforcement": {
            "contract_field": contract["resume_enforcement"]["contract_field"].clone(),
            "resume_state_field": contract["resume_enforcement"]["resume_state_field"].clone(),
            "obligation_field": contract["resume_enforcement"]["obligation_field"].clone(),
            "startup_next_action_field": contract["resume_enforcement"]["startup_next_action_field"].clone(),
            "active_lease_field": contract["resume_enforcement"]["active_lease_field"].clone(),
            "active_lease_owner_state_field": contract["resume_enforcement"]["active_lease_owner_state_field"].clone(),
            "previous_session_owner_value": contract["resume_enforcement"]["previous_session_owner_value"].clone(),
            "must_resume_required_return_task_before_unrelated_work": contract["resume_enforcement"]["must_resume_required_return_task_before_unrelated_work"].clone(),
            "previous_session_owner_must_follow_startup_next_action": contract["resume_enforcement"]["previous_session_owner_must_follow_startup_next_action"].clone(),
            "required_action_kind_when_resume_required": contract["resume_enforcement"]["required_action_kind_when_resume_required"].clone(),
            "no_silent_drop": contract["resume_enforcement"]["no_silent_drop"].clone()
        },
        "startup_execution_gate_enforcement": {
            "must_follow_field": contract["startup_execution_gate_enforcement"]["must_follow_field"].clone(),
            "unrelated_work_allowed_field": contract["startup_execution_gate_enforcement"]["unrelated_work_allowed_field"].clone(),
            "must_read_prompt_text_before_reply_field": contract["startup_execution_gate_enforcement"]["must_read_prompt_text_before_reply_field"].clone(),
            "required_action_kind_field": contract["startup_execution_gate_enforcement"]["required_action_kind_field"].clone(),
            "no_silent_drop_field": contract["startup_execution_gate_enforcement"]["no_silent_drop_field"].clone()
        },
        "live_client_budget_enforcement": {
            "guard_command": contract["live_client_budget_enforcement"]["guard_command"].clone(),
            "guard_summary_field": contract["live_client_budget_enforcement"]["guard_summary_field"].clone(),
            "reply_execution_gate_field": contract["live_client_budget_enforcement"]["reply_execution_gate_field"].clone(),
            "reply_execution_gate_version": contract["live_client_budget_enforcement"]["reply_execution_gate_version"].clone(),
            "reply_budget_mode_field": contract["live_client_budget_enforcement"]["reply_budget_mode_field"].clone(),
            "reply_budget_contract_field": contract["live_client_budget_enforcement"]["reply_budget_contract_field"].clone(),
            "compact_reply_mode_value": contract["live_client_budget_enforcement"]["compact_reply_mode_value"].clone(),
            "compact_reply_contract_version": contract["live_client_budget_enforcement"]["compact_reply_contract_version"].clone(),
            "compact_diagnostics_command": contract["live_client_budget_enforcement"]["compact_diagnostics_command"].clone(),
            "must_prefer_compact_diagnostics_over_full_snapshot": contract["live_client_budget_enforcement"]["must_prefer_compact_diagnostics_over_full_snapshot"].clone(),
            "guard_enforcement_flag": contract["live_client_budget_enforcement"]["guard_enforcement_flag"].clone(),
            "guard_enforcement_exit_on_blocking": contract["live_client_budget_enforcement"]["guard_enforcement_exit_on_blocking"].clone(),
            "must_check_before_each_substantive_reply": contract["live_client_budget_enforcement"]["must_check_before_each_substantive_reply"].clone(),
            "max_guard_age_seconds": contract["live_client_budget_enforcement"]["max_guard_age_seconds"].clone(),
            "stale_guard_requires_refresh": contract["live_client_budget_enforcement"]["stale_guard_requires_refresh"].clone(),
            "rotate_now_field": contract["live_client_budget_enforcement"]["rotate_now_field"].clone(),
            "status_label_field": contract["live_client_budget_enforcement"]["status_label_field"].clone(),
            "rotate_status_labels": contract["live_client_budget_enforcement"]["rotate_status_labels"].clone(),
            "save_handoff_before_rotate": contract["live_client_budget_enforcement"]["save_handoff_before_rotate"].clone(),
            "fresh_chat_requires_continuity_startup": contract["live_client_budget_enforcement"]["fresh_chat_requires_continuity_startup"].clone(),
            "full_scale_client_truth_required": contract["live_client_budget_enforcement"]["full_scale_client_truth_required"].clone(),
            "blocking_reply_contract_field": contract["live_client_budget_enforcement"]["blocking_reply_contract_field"].clone(),
            "blocking_reply_contract_version": contract["live_client_budget_enforcement"]["blocking_reply_contract_version"].clone(),
            "blocking_reply_response_kind": contract["live_client_budget_enforcement"]["blocking_reply_response_kind"].clone(),
            "blocking_reply_max_sentences": contract["live_client_budget_enforcement"]["blocking_reply_max_sentences"].clone(),
            "blocking_reply_must_avoid_substantive_work": contract["live_client_budget_enforcement"]["blocking_reply_must_avoid_substantive_work"].clone(),
            "blocking_reply_must_use_action_bundle_operator_flow": contract["live_client_budget_enforcement"]["blocking_reply_must_use_action_bundle_operator_flow"].clone(),
            "blocking_reply_template": contract["live_client_budget_enforcement"]["blocking_reply_template"].clone()
        },
        "fail_closed_conditions": contract["fail_closed_conditions"].clone()
    });
    serde_json::to_string_pretty(&payload)
        .context("failed to serialize startup agent contract artifact")
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
        InstallState, detection_score, env_keys, expand_target_template, inspect_startup_artifacts,
        install_scope_status, merge_managed_startup_block, render_startup_agent_contract_artifact,
        render_startup_contract_artifact, render_startup_instructions, resolve_client_target,
        resolve_output_path, save_install_state, startup_agent_contract_artifact_path,
        startup_contract_artifact_path, startup_contract_sha256, strip_managed_startup_block,
        working_state_reason_summary,
    };
    use crate::mcp;
    use crate::working_state;
    use serde_json::{Value, json};
    use std::fs;
    use std::path::{Path, PathBuf};

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
        let text = render_startup_instructions(repo, "VS Code", "vscode", "vscode_instructions_md")
            .expect("startup instructions must render");
        assert!(text.contains("AMAI MANAGED STARTUP INSTRUCTIONS v1"));
        assert!(text.contains(
            "Перед первым содержательным ответом в новом или resumed чате, а затем перед каждым следующим содержательным ответом, пока этот чат жив:"
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
        assert!(text.contains("continuity startup-state --repo-root"));
        let expected_sha = startup_contract_sha256(&mcp::project_chat_startup_contract())
            .expect("startup contract hash");
        assert!(text.contains(expected_sha.as_str()));
        assert!(text.contains("previous_session_owner_must_follow_startup_next_action = true"));
        assert!(text.contains("no_silent_drop = true"));
        assert!(text.contains("cargo run -- observe client-budget-gate"));
        assert!(text.contains("must_check_before_each_substantive_reply = true"));
        assert!(text.contains("--enforce-reply-gate"));
        assert!(text.contains("guard_enforcement_exit_on_blocking = true"));
        assert!(text.contains("cargo run -- observe client-budget-root-cause"));
        assert!(text.contains("must_prefer_compact_diagnostics_over_full_snapshot = true"));
        assert!(text.contains("client_budget_reply_gate.reply_execution_gate"));
        assert!(text.contains("Gate version pinned: `client-reply-budget-gate-v1`"));
        assert!(text.contains("reply_budget_mode == \"compact_high_signal\""));
        assert!(text.contains("reply_budget_contract"));
        assert!(text.contains("contract_version = \"client-reply-budget-v1\""));
        assert!(text.contains("direct answer first"));
        assert!(text.contains("no unrequested recap"));
        assert!(text.contains("no repeated known context"));
        assert!(text.contains("stale старше `10` секунд"));
        assert!(text.contains("stale_guard_requires_refresh = true"));
        assert!(text.contains("blocking_reply_contract"));
        assert!(text.contains("contract_version = \"client-budget-blocked-reply-v1\""));
        assert!(text.contains("response_kind = \"rotate_chat_only\""));
        assert!(text.contains("max_sentences = 1"));
        assert!(text.contains("must_avoid_substantive_work = true"));
        assert!(text.contains("must_use_action_bundle_operator_flow = true"));
        assert!(text.contains("новый чат нужен сейчас"));
        assert!(text.contains("full_scale_client_truth_required = true"));
        assert!(text.contains("внутренним Amai-slice"));
        assert!(text.len() < 7000);
    }

    #[test]
    fn renders_machine_readable_startup_contract_artifact() {
        let repo = Path::new("/tmp/amai");
        let (text, sha256) =
            render_startup_contract_artifact(repo).expect("startup contract must render");
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
            json!(working_state::CLIENT_BUDGET_BLOCKING_REPLY_RESPONSE_KIND)
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
            json!("continuity-startup-contract-v13")
        );
        assert_eq!(
            payload["startup_contract"]["purpose"],
            json!(
                "project-scoped continuity restore plus live client-budget discipline before each substantive reply in a new, resumed, or ongoing chat"
            )
        );
        assert_eq!(
            payload["startup_contract"]["live_client_budget_enforcement"]["guard_command"],
            json!("observe client-budget-gate")
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
            payload["startup_contract"]["live_client_budget_enforcement"]["max_guard_age_seconds"],
            json!(10)
        );
        assert_eq!(
            payload["startup_contract"]["live_client_budget_enforcement"]["stale_guard_requires_refresh"],
            json!(true)
        );
    }

    #[test]
    fn renders_compact_startup_agent_contract_artifact() {
        let repo = Path::new("/tmp/amai");
        let (full_text, sha256) =
            render_startup_contract_artifact(repo).expect("startup contract must render");
        let text = render_startup_agent_contract_artifact(repo)
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
            payload["live_client_budget_enforcement"]["compact_diagnostics_command"],
            json!("observe client-budget-root-cause")
        );
        assert!(
            text.len() < full_text.len(),
            "startup agent contract must be smaller than full contract"
        );
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
            render_startup_contract_artifact(&repo).expect("startup contract");
        fs::write(&startup_contract_path, contract_text).expect("write startup contract");

        let startup_instruction_path =
            repo.join(".github/instructions/amai-continuity-startup.instructions.md");
        if let Some(parent) = startup_instruction_path.parent() {
            fs::create_dir_all(parent).expect("startup instruction dir");
        }
        let startup_instructions =
            render_startup_instructions(&repo, "VS Code", "vscode", "vscode_instructions_md")
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
    fn managed_startup_block_appends_to_existing_rules_file() {
        let repo = Path::new("/tmp/amai");
        let block = render_startup_instructions(repo, "Codex", "codex", "codex_agents_snippet")
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
        let block = render_startup_instructions(repo, "Codex", "codex", "codex_agents_snippet")
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
