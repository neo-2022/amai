use crate::cli::{BootstrapDisconnectArgs, BootstrapOnboardingArgs, McpConfigArgs};
use crate::config;
use crate::mcp;
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

pub async fn run(args: &BootstrapOnboardingArgs) -> Result<()> {
    let repo_root = discover_repo_root(args.cwd.as_deref())?;
    let remote_mode = args.ssh_destination.is_some();
    let client_resolution = resolve_client_target(&repo_root, &args.client)?;
    let package_version = env!("CARGO_PKG_VERSION").to_string();
    let repo_revision = current_repo_revision(&repo_root).await;
    let mut local_preflight_report: Option<profiles::PreflightReport> = None;

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

    if !remote_mode {
        let report = profiles::preflight_report(&repo_root, &args.stack_profile)?;
        if env::var("AMAI_PREFLIGHT_ALREADY_SHOWN").unwrap_or_default() != "1" {
            profiles::print_preflight_report(&report);
        }
        confirm_local_installation(args, &repo_root, &client_resolution, &report)?;
        local_preflight_report = Some(report);
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
    if let Some(backup) = backup {
        println!("Резервная копия старого config: {}", backup.display());
    }
    if remote_mode {
        println!("Что делать дальше:");
        println!("- проверьте, что SSH до сервера работает");
        println!("- перезапустите клиент или сделайте Reload Window");
        println!("- попросите клиента обратиться к Amai через MCP");
    } else {
        if let Some(report) = &local_preflight_report {
            println!();
            println!("Сводка по этой машине:");
            println!("- CPU: {} логических потоков", report.host_logical_cpus);
            println!(
                "- Память: {:.2} GiB всего, свободно сейчас {:.2} GiB",
                report.host_total_memory_gib, report.host_available_memory_gib
            );
            println!("- Диск: свободно {:.2} GiB", report.host_available_disk_gib);
            println!(
                "- Итог по выбранному режиму: {}",
                human_verdict(report.verdict)
            );
            println!("На что можно рассчитывать:");
            for item in &report.profile.suitable_for {
                println!("- {}", item);
            }
            if report.verdict != "pass" {
                println!("Предупреждение:");
                println!(
                    "- этот режим запускается, но не даёт большого запаса по тяжёлым сценариям"
                );
            } else if !report.profile.supports_peak_benchmarks {
                println!("Важно:");
                println!(
                    "- выбран лёгкий режим. Он подходит для удалённого доступа и smoke/demo, но не для рекордных benchmark-цифр"
                );
            }
        }
        println!("Что делать дальше:");
        println!("- откройте репозиторий в клиенте");
        println!("- перезапустите клиент или сделайте Reload Window");
        println!("- попросите клиента обратиться к Amai через MCP");
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

fn human_verdict(verdict: &str) -> &'static str {
    match verdict {
        "pass" => "машина подходит",
        "warn" => "машина подходит с оговорками",
        "fail" => "машина не подходит",
        _ => "статус не определён",
    }
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
    let client_resolution = resolve_client_target(&repo_root, &args.client)?;
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
}

fn load_client_targets_manifest(repo_root: &Path) -> Result<ClientTargetsManifest> {
    let manifest_path = repo_root.join("config/client_targets.toml");
    let content = fs::read_to_string(&manifest_path)
        .with_context(|| format!("failed to read {}", manifest_path.display()))?;
    toml::from_str(&content).context("failed to parse config/client_targets.toml")
}

fn resolve_client_target(repo_root: &Path, requested_client: &str) -> Result<ClientResolution> {
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
        });
    }

    let home = dirs::home_dir().ok_or_else(|| anyhow!("failed to resolve user home directory"))?;
    let mut best: Option<(String, ClientTarget, i64, String)> = None;
    for (client_key, target) in &manifest.clients {
        if let Some(score) = detection_score(repo_root, &home, target) {
            let reason = detection_reason(repo_root, &home, target)
                .unwrap_or_else(|| "auto_detected".to_string());
            match &best {
                Some((_, _, best_score, _)) if score <= *best_score => {}
                _ => {
                    best = Some((client_key.clone(), target.clone(), score, reason));
                }
            }
        }
    }

    if let Some((client_key, target, _, reason)) = best {
        return Ok(ClientResolution {
            client_key,
            target,
            auto_selected: true,
            reason,
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
    })
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
        let target = resolve_client_target(repo, "vscode")
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
        let target = resolve_client_target(repo, "vscode").unwrap().target;
        let explicit = repo.join("custom/mcp.json");
        assert_eq!(
            resolve_output_path(repo, &target, Some(&explicit)).unwrap(),
            explicit
        );
    }

    #[test]
    fn resolve_client_target_auto_prefers_workspace_marker() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
        let resolution = resolve_client_target(repo, "auto").unwrap();
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
