use crate::cli::{BootstrapDisconnectArgs, BootstrapOnboardingArgs, McpConfigArgs};
use crate::config;
use crate::mcp;
use crate::observe;
use crate::profiles;
use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
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
}

pub async fn run(args: &BootstrapOnboardingArgs) -> Result<()> {
    let repo_root = discover_repo_root(args.cwd.as_deref())?;
    let remote_mode = args.ssh_destination.is_some();
    let client_prompt_allowed = env::var("AMAI_ALLOW_CLIENT_PROMPT").unwrap_or_default() == "1"
        || (!args.yes && interactive_prompt_allowed());
    let client_resolution = resolve_client_target(&repo_root, &args.client, client_prompt_allowed)?;
    let package_version = env!("CARGO_PKG_VERSION").to_string();
    let repo_revision = current_repo_revision(&repo_root).await;
    let mut local_preflight_report: Option<profiles::PreflightReport> = None;
    let mut local_machine_summary: Option<InstallMachineSummary> = None;
    let mut local_metrics_summary: Option<InstallMetricsSummary> = None;

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

        if let Ok(cfg) = config::AppConfig::from_env() {
            local_metrics_summary = collect_install_metrics_summary(&cfg).await.ok();
        }
    }
    let backup = maybe_backup_user_global(&output, &target.install_scope)?;
    mcp::write_client_config(&config_args)?;
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
        },
    )?;

    let release_binary = repo_root.join("target/release/amai");
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
    })
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
    let client_resolution = resolve_client_target(&repo_root, &args.client, false)?;
    let target = client_resolution.target.clone();
    let output = resolve_output_path(&repo_root, &target, args.output.as_ref())?;
    let backup = maybe_backup_user_global(&output, &target.install_scope)?;

    let result = mcp::remove_client_config(
        &McpConfigArgs {
            client: client_resolution.client_key.clone(),
            server_name: "amai".to_string(),
            launcher_platform: "auto".to_string(),
            ssh_destination: None,
            remote_repo_root: None,
            command: None,
            cwd: Some(repo_root),
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
    if let Some(backup) = backup {
        println!("backup_file: {}", backup.display());
    }
    println!("next_step_1: reload the client window or restart the client");
    println!("next_step_2: verify that Amai is no longer listed as an MCP server");
    Ok(())
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
        detection_score, env_keys, expand_target_template, install_scope_status,
        resolve_client_target, resolve_output_path,
    };
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
        };
        let repo = PathBuf::from("/tmp/amai-nonexistent");
        let home = PathBuf::from("/tmp/amai-home-nonexistent");
        assert!(detection_score(&repo, &home, &target).is_none());
    }
}
