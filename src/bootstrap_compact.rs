use crate::bootstrap;
use crate::config;
use crate::profiles;
use anyhow::{Context, Result, anyhow, bail};
use dirs::home_dir;
use serde::Serialize;
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io::{self, IsTerminal};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::process::Command;

#[derive(Debug, Clone)]
pub struct CompactBootstrapArgs {
    pub command: CompactBootstrapCommand,
}

#[derive(Debug, Clone)]
pub enum CompactBootstrapCommand {
    Preflight {
        stack_profile: String,
    },
    Stack,
    Install {
        client: String,
        stack_profile: String,
        yes: bool,
        output: Option<PathBuf>,
        cwd: Option<PathBuf>,
        skip_stack: bool,
        ssh_destination: Option<String>,
        remote_repo_root: Option<PathBuf>,
    },
    Remove {
        client: String,
        output: Option<PathBuf>,
        cwd: Option<PathBuf>,
        purge_empty_file: bool,
    },
}

#[derive(Debug, Clone, Serialize)]
struct CompactInstallState {
    package_version: String,
    repo_revision: String,
    client_key: String,
    client_config: String,
    stack_profile: String,
    installed_at_epoch_seconds: u64,
}

#[derive(Debug, Clone, Serialize)]
struct VscodeMcpServerConfig {
    args: Vec<String>,
    command: String,
    cwd: String,
    #[serde(rename = "type")]
    server_type: String,
}

#[derive(Debug, Clone, Serialize)]
struct VscodeMcpConfig {
    servers: BTreeMap<String, VscodeMcpServerConfig>,
}

pub async fn run(args: CompactBootstrapArgs) -> Result<()> {
    match args.command {
        CompactBootstrapCommand::Preflight { stack_profile } => {
            let repo_root = config::discover_repo_root(None)?;
            profiles::print_preflight(&repo_root, &stack_profile)?;
        }
        CompactBootstrapCommand::Stack => {
            load_env_contour();
            let cfg = config::AppConfig::from_env()?;
            bootstrap::bootstrap_stack(&cfg).await?;
        }
        CompactBootstrapCommand::Install {
            client,
            stack_profile,
            yes,
            output,
            cwd,
            skip_stack,
            ssh_destination,
            remote_repo_root,
        } => {
            install(
                &client,
                &stack_profile,
                yes,
                output.as_ref(),
                cwd.as_deref(),
                skip_stack,
                ssh_destination.as_deref(),
                remote_repo_root.as_deref(),
            )
            .await?;
        }
        CompactBootstrapCommand::Remove {
            client,
            output,
            cwd,
            purge_empty_file,
        } => {
            remove(&client, output.as_ref(), cwd.as_deref(), purge_empty_file).await?;
        }
    }
    Ok(())
}

async fn install(
    client: &str,
    stack_profile: &str,
    yes: bool,
    output: Option<&PathBuf>,
    cwd: Option<&Path>,
    skip_stack: bool,
    ssh_destination: Option<&str>,
    remote_repo_root: Option<&Path>,
) -> Result<()> {
    if ssh_destination.is_some() || remote_repo_root.is_some() {
        bail!("compact bootstrap install currently supports only local hosts");
    }
    let client_key = normalize_client_key(client)?;
    if client_key != "vscode" {
        bail!("compact bootstrap install currently supports only VS Code/Codium");
    }

    let repo_root = config::discover_repo_root(cwd)?;
    best_effort_cleanup_mcp_orphans(&repo_root).await;

    if !yes && interactive_prompt_allowed() {
        println!("Amai compact install path continues with VS Code/Codium only.");
    }

    ensure_local_config_files(&repo_root)?;
    dotenvy::from_path_override(repo_root.join(".env"))
        .context("failed to load generated .env for compact bootstrap install")?;

    check_dependency("docker", &["--version"]).await?;
    check_dependency("code", &["--version"]).await?;

    if !skip_stack {
        let mut bootstrap_stack = script_command(
            &repo_root,
            "scripts/bootstrap_stack.sh",
            ["--stack-profile", stack_profile],
        );
        bootstrap_stack.env("AMAI_SKIP_STACK_PREFLIGHT", "1");
        run_command("bootstrap stack", bootstrap_stack).await?;
    }

    run_command(
        "install managed stack autostart",
        script_command(&repo_root, "scripts/install_stack_autostart.sh", []),
    )
    .await?;

    run_command(
        "install vscode bridge",
        script_command(&repo_root, "scripts/install_vscode_amai_bridge.sh", []),
    )
    .await?;

    let client_config_path = resolve_vscode_output_path(&repo_root, output);
    write_vscode_mcp_config(&repo_root, &client_config_path)?;

    let install_state = CompactInstallState {
        package_version: env!("CARGO_PKG_VERSION").to_string(),
        repo_revision: current_repo_revision(&repo_root).await,
        client_key: client_key.to_string(),
        client_config: client_config_path.display().to_string(),
        stack_profile: stack_profile.to_string(),
        installed_at_epoch_seconds: current_epoch_seconds(),
    };
    save_install_state(&repo_root, &install_state)?;

    println!("Amai готов");
    println!("Версия Amai: {}", install_state.package_version);
    println!("Ревизия сборки: {}", install_state.repo_revision);
    println!("Режим подключения: локальный");
    println!("Клиент: VS Code / Codium");
    println!("Файл окружения: {}", repo_root.join(".env").display());
    println!("Файл подключения: {}", client_config_path.display());
    println!("Выбранный профиль: {}", stack_profile);
    println!("Внешний memory bridge: пропущен в compact install contour");
    println!("Startup contract для клиента: пропущен в compact install contour");
    println!("Client runtime artifact: VS Code bridge установлен");
    println!("Release binary готов: нет");
    println!("Что делать дальше:");
    println!("- откройте VS Code или Codium в каталоге репозитория");
    println!("- сделайте Reload Window");
    println!("- убедитесь, что MCP server `amai` виден клиенту");
    Ok(())
}

async fn remove(
    client: &str,
    output: Option<&PathBuf>,
    cwd: Option<&Path>,
    purge_empty_file: bool,
) -> Result<()> {
    let client_key = normalize_client_key(client)?;
    if client_key != "vscode" {
        bail!("compact bootstrap remove currently supports only VS Code/Codium");
    }

    let repo_root = config::discover_repo_root(cwd)?;
    best_effort_cleanup_mcp_orphans(&repo_root).await;

    let client_config_path = resolve_vscode_output_path(&repo_root, output);
    let removed = remove_vscode_mcp_config(&client_config_path, purge_empty_file)?;
    let bridge_removed = remove_vscode_bridge_install().await?;
    let startup_instruction_path =
        repo_root.join(".github/instructions/amai-continuity-startup.instructions.md");
    let startup_instruction_removed = if startup_instruction_path.is_file() {
        fs::remove_file(&startup_instruction_path)
            .with_context(|| format!("failed to remove {}", startup_instruction_path.display()))?;
        true
    } else {
        false
    };
    let startup_contract_path = repo_root.join(".amai/onboarding/project-chat-startup-contract.json");
    if startup_contract_path.is_file() {
        fs::remove_file(&startup_contract_path)
            .with_context(|| format!("failed to remove {}", startup_contract_path.display()))?;
    }

    println!("disconnect completed");
    println!("client: vscode");
    println!("client_display_name: VS Code");
    println!("client_resolution_mode: compact_vscode_only");
    println!("client_detection_reason: compact_vscode_only");
    println!("client_config: {}", client_config_path.display());
    println!("server_removed: {}", removed.removed);
    println!("file_purged: {}", removed.purged_file);
    println!("startup_instruction_removed: {}", startup_instruction_removed);
    println!("client_runtime_removed: {}", bridge_removed);

    if !full_remove_mode_enabled() {
        println!("next_step_1: reload the client window or restart the client");
        println!("next_step_2: verify that Amai is no longer listed as an MCP server");
        return Ok(());
    }

    let managed_clone_root = managed_clone_root()?;
    let install_state_path = install_state_path(&repo_root);
    let install_state_present_before = install_state_path.exists();
    let systemd_user_unit_removed = remove_stack_autostart_unit().await?;
    let stack_down_succeeded = compose_down_stack(&repo_root).await?;

    let state_dir = repo_root.join("state");
    let tmp_dir = repo_root.join("tmp");
    let mut state_tree_removed = false;
    state_tree_removed |= remove_tree_forcefully(&state_dir).await?;
    state_tree_removed |= remove_tree_forcefully(&tmp_dir).await?;

    let mut install_state_removed = false;
    if install_state_path.exists() {
        fs::remove_file(&install_state_path)
            .with_context(|| format!("failed to remove {}", install_state_path.display()))?;
        install_state_removed = true;
    } else if install_state_present_before {
        install_state_removed = true;
    }

    let mut repo_root_removed = false;
    let mut support_root_removed = false;
    if repo_root == managed_clone_root {
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
        if remove_tree_forcefully(&repo_root).await? {
            repo_root_removed = true;
        }
        if let Some(parent) = managed_clone_root.parent()
            && parent.exists()
            && is_directory_empty(parent)?
        {
            fs::remove_dir(parent)
                .with_context(|| format!("failed to remove empty {}", parent.display()))?;
            support_root_removed = true;
        }
    }

    println!("full_remove: true");
    println!("systemd_user_unit_removed: {}", systemd_user_unit_removed);
    println!("stack_down_succeeded: {}", stack_down_succeeded);
    println!("state_tree_removed: {}", state_tree_removed);
    println!("install_state_removed: {}", install_state_removed);
    println!("repo_root_removed: {}", repo_root_removed);
    println!("support_root_removed: {}", support_root_removed);
    println!("vscode_bridge_removed: {}", bridge_removed);
    println!("next_step_1: verify that Amai is no longer installed on this host");
    println!("next_step_2: reinstall from GitHub only if you intentionally want Amai back");
    Ok(())
}

fn normalize_client_key(raw: &str) -> Result<&str> {
    let normalized = raw.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "" | "auto" | "vscode" => Ok("vscode"),
        other => bail!("unsupported compact bootstrap client target: {other}"),
    }
}

fn resolve_vscode_output_path(repo_root: &Path, explicit: Option<&PathBuf>) -> PathBuf {
    explicit
        .cloned()
        .unwrap_or_else(|| repo_root.join(".vscode/mcp.json"))
}

fn write_vscode_mcp_config(repo_root: &Path, output: &Path) -> Result<()> {
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let mut servers = BTreeMap::new();
    servers.insert(
        "amai".to_string(),
        VscodeMcpServerConfig {
            args: Vec::new(),
            command: repo_root
                .join("scripts/run_mcp_stdio.sh")
                .display()
                .to_string(),
            cwd: repo_root.display().to_string(),
            server_type: "stdio".to_string(),
        },
    );
    let payload = VscodeMcpConfig { servers };
    fs::write(output, serde_json::to_string_pretty(&payload)? + "\n")
        .with_context(|| format!("failed to write {}", output.display()))?;
    Ok(())
}

struct RemoveConfigResult {
    removed: bool,
    purged_file: bool,
}

fn remove_vscode_mcp_config(output: &Path, purge_empty_file: bool) -> Result<RemoveConfigResult> {
    if !output.exists() {
        return Ok(RemoveConfigResult {
            removed: false,
            purged_file: false,
        });
    }
    let existing = fs::read_to_string(output)
        .with_context(|| format!("failed to read {}", output.display()))?;
    let mut payload: serde_json::Value = serde_json::from_str(&existing)
        .with_context(|| format!("failed to parse {}", output.display()))?;
    let Some(servers) = payload
        .get_mut("servers")
        .and_then(|value| value.as_object_mut())
    else {
        return Ok(RemoveConfigResult {
            removed: false,
            purged_file: false,
        });
    };
    let removed = servers.remove("amai").is_some();
    if !removed {
        return Ok(RemoveConfigResult {
            removed: false,
            purged_file: false,
        });
    }
    let is_empty = servers.is_empty();
    if purge_empty_file && is_empty {
        fs::remove_file(output)
            .with_context(|| format!("failed to remove {}", output.display()))?;
        return Ok(RemoveConfigResult {
            removed: true,
            purged_file: true,
        });
    }
    fs::write(output, serde_json::to_string_pretty(&payload)? + "\n")
        .with_context(|| format!("failed to write {}", output.display()))?;
    Ok(RemoveConfigResult {
        removed: true,
        purged_file: false,
    })
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
            if raw.trim().is_empty() {
                continue;
            }
            let mut entries: Vec<serde_json::Value> = serde_json::from_str(&raw)
                .context("failed to parse VS Code extensions registry")?;
            let before_len = entries.len();
            entries.retain(|entry| {
                let Some(identifier) = entry.get("identifier") else {
                    return true;
                };
                let Some(id) = identifier.get("id").and_then(serde_json::Value::as_str) else {
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

fn load_env_contour() {
    dotenvy::dotenv().ok();
    if std::env::var_os("AMI_STACK_NAME").is_some() {
        return;
    }
    let manifest_env = Path::new(env!("CARGO_MANIFEST_DIR")).join(".env");
    dotenvy::from_path_override(&manifest_env).ok();
}

fn install_state_path(repo_root: &Path) -> PathBuf {
    env::var_os("AMAI_INSTALL_STATE_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_root.join("state/install_state.json"))
}

fn save_install_state(repo_root: &Path, state: &CompactInstallState) -> Result<()> {
    let path = install_state_path(repo_root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let text = serde_json::to_string_pretty(state).context("failed to serialize install state")?;
    fs::write(&path, text + "\n").with_context(|| format!("failed to write {}", path.display()))
}

fn current_epoch_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

async fn current_repo_revision(repo_root: &Path) -> String {
    match Command::new("git")
        .args(["rev-parse", "HEAD"])
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

fn managed_clone_root() -> Result<PathBuf> {
    if let Some(explicit) = env::var_os("AMAI_GITHUB_CLONE_DIR") {
        return Ok(PathBuf::from(explicit));
    }
    let home = home_dir().ok_or_else(|| anyhow!("failed to resolve home directory"))?;
    Ok(home.join(".local/share/amai/repo"))
}

fn full_remove_mode_enabled() -> bool {
    matches!(
        env::var("AMAI_BOOTSTRAP_REMOVE_MODE").ok().as_deref(),
        Some("1" | "true" | "TRUE" | "full" | "FULL")
    )
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

fn env_keys(content: &str) -> std::collections::BTreeSet<String> {
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
        bail!("{program} is required for compact bootstrap but is not available");
    }
    Ok(())
}

async fn best_effort_cleanup_mcp_orphans(repo_root: &Path) {
    let script_path = repo_root.join("scripts/cleanup_mcp_orphans.sh");
    if !script_path.is_file() {
        return;
    }

    let _ = Command::new(&script_path)
        .arg(repo_root)
        .current_dir(repo_root)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await;
}

fn interactive_prompt_allowed() -> bool {
    env::var("AMAI_FORCE_INTERACTIVE_PROMPT").unwrap_or_default() == "1"
        || (io::stdin().is_terminal() && io::stdout().is_terminal())
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

pub fn parse_args<I>(args: I) -> Result<CompactBootstrapArgs>
where
    I: IntoIterator<Item = String>,
{
    let mut iter = args.into_iter();
    let command = iter
        .next()
        .ok_or_else(|| anyhow!("missing compact bootstrap command"))?;
    match command.as_str() {
        "preflight" => {
            let mut stack_profile = "default".to_string();
            let rest = iter.collect::<Vec<_>>();
            let mut index = 0usize;
            while index < rest.len() {
                match rest[index].as_str() {
                    "--stack-profile" => {
                        let value = rest
                            .get(index + 1)
                            .ok_or_else(|| anyhow!("missing value for --stack-profile"))?;
                        stack_profile = value.clone();
                        index += 2;
                    }
                    other => bail!("unsupported compact preflight argument: {other}"),
                }
            }
            Ok(CompactBootstrapArgs {
                command: CompactBootstrapCommand::Preflight { stack_profile },
            })
        }
        "stack" => Ok(CompactBootstrapArgs {
            command: CompactBootstrapCommand::Stack,
        }),
        "install" => {
            let mut client = "auto".to_string();
            let mut stack_profile = "default".to_string();
            let mut yes = false;
            let mut output = None;
            let mut cwd = None;
            let mut skip_stack = false;
            let mut ssh_destination = None;
            let mut remote_repo_root = None;
            let rest = iter.collect::<Vec<_>>();
            let mut index = 0usize;
            while index < rest.len() {
                match rest[index].as_str() {
                    "--client" => {
                        client = rest
                            .get(index + 1)
                            .ok_or_else(|| anyhow!("missing value for --client"))?
                            .clone();
                        index += 2;
                    }
                    value if value.starts_with("--client=") => {
                        client = value["--client=".len()..].to_string();
                        index += 1;
                    }
                    "--stack-profile" => {
                        stack_profile = rest
                            .get(index + 1)
                            .ok_or_else(|| anyhow!("missing value for --stack-profile"))?
                            .clone();
                        index += 2;
                    }
                    value if value.starts_with("--stack-profile=") => {
                        stack_profile = value["--stack-profile=".len()..].to_string();
                        index += 1;
                    }
                    "--yes" => {
                        yes = true;
                        index += 1;
                    }
                    "--output" => {
                        output = Some(PathBuf::from(
                            rest.get(index + 1)
                                .ok_or_else(|| anyhow!("missing value for --output"))?,
                        ));
                        index += 2;
                    }
                    value if value.starts_with("--output=") => {
                        output = Some(PathBuf::from(&value["--output=".len()..]));
                        index += 1;
                    }
                    "--cwd" => {
                        cwd = Some(PathBuf::from(
                            rest.get(index + 1)
                                .ok_or_else(|| anyhow!("missing value for --cwd"))?,
                        ));
                        index += 2;
                    }
                    value if value.starts_with("--cwd=") => {
                        cwd = Some(PathBuf::from(&value["--cwd=".len()..]));
                        index += 1;
                    }
                    "--skip-stack" => {
                        skip_stack = true;
                        index += 1;
                    }
                    "--skip-release-build" => {
                        index += 1;
                    }
                    "--launcher-platform" => {
                        index += 2;
                    }
                    value if value.starts_with("--launcher-platform=") => {
                        index += 1;
                    }
                    "--workspace-root" => {
                        index += 2;
                    }
                    value if value.starts_with("--workspace-root=") => {
                        index += 1;
                    }
                    "--ssh-destination" => {
                        ssh_destination = Some(
                            rest.get(index + 1)
                                .ok_or_else(|| anyhow!("missing value for --ssh-destination"))?
                                .clone(),
                        );
                        index += 2;
                    }
                    value if value.starts_with("--ssh-destination=") => {
                        ssh_destination = Some(value["--ssh-destination=".len()..].to_string());
                        index += 1;
                    }
                    "--remote-repo-root" => {
                        remote_repo_root =
                            Some(PathBuf::from(rest.get(index + 1).ok_or_else(|| {
                                anyhow!("missing value for --remote-repo-root")
                            })?));
                        index += 2;
                    }
                    value if value.starts_with("--remote-repo-root=") => {
                        remote_repo_root =
                            Some(PathBuf::from(&value["--remote-repo-root=".len()..]));
                        index += 1;
                    }
                    other => bail!("unsupported compact install argument: {other}"),
                }
            }
            Ok(CompactBootstrapArgs {
                command: CompactBootstrapCommand::Install {
                    client,
                    stack_profile,
                    yes,
                    output,
                    cwd,
                    skip_stack,
                    ssh_destination,
                    remote_repo_root,
                },
            })
        }
        "remove" => {
            let mut client = "auto".to_string();
            let mut output = None;
            let mut cwd = None;
            let mut purge_empty_file = true;
            let rest = iter.collect::<Vec<_>>();
            let mut index = 0usize;
            while index < rest.len() {
                match rest[index].as_str() {
                    "--client" => {
                        client = rest
                            .get(index + 1)
                            .ok_or_else(|| anyhow!("missing value for --client"))?
                            .clone();
                        index += 2;
                    }
                    value if value.starts_with("--client=") => {
                        client = value["--client=".len()..].to_string();
                        index += 1;
                    }
                    "--output" => {
                        output = Some(PathBuf::from(
                            rest.get(index + 1)
                                .ok_or_else(|| anyhow!("missing value for --output"))?,
                        ));
                        index += 2;
                    }
                    value if value.starts_with("--output=") => {
                        output = Some(PathBuf::from(&value["--output=".len()..]));
                        index += 1;
                    }
                    "--cwd" => {
                        cwd = Some(PathBuf::from(
                            rest.get(index + 1)
                                .ok_or_else(|| anyhow!("missing value for --cwd"))?,
                        ));
                        index += 2;
                    }
                    value if value.starts_with("--cwd=") => {
                        cwd = Some(PathBuf::from(&value["--cwd=".len()..]));
                        index += 1;
                    }
                    "--purge-empty-file=false" => {
                        purge_empty_file = false;
                        index += 1;
                    }
                    "--purge-empty-file=true" | "--purge-empty-file" => {
                        purge_empty_file = true;
                        index += 1;
                    }
                    other => bail!("unsupported compact remove argument: {other}"),
                }
            }
            Ok(CompactBootstrapArgs {
                command: CompactBootstrapCommand::Remove {
                    client,
                    output,
                    cwd,
                    purge_empty_file,
                },
            })
        }
        other => bail!("unsupported compact bootstrap command: {other}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_install_accepts_vscode_defaults() {
        let args = parse_args(vec![
            "install".to_string(),
            "--client".to_string(),
            "vscode".to_string(),
            "--stack-profile".to_string(),
            "default".to_string(),
            "--yes".to_string(),
        ])
        .expect("parse install");
        let CompactBootstrapCommand::Install {
            client,
            stack_profile,
            yes,
            ..
        } = args.command
        else {
            panic!("expected install command");
        };
        assert_eq!(client, "vscode");
        assert_eq!(stack_profile, "default");
        assert!(yes);
    }

    #[test]
    fn remove_vscode_mcp_config_purges_empty_file() {
        let unique = format!(
            "amai-bootstrap-test-{}-{}",
            std::process::id(),
            current_epoch_seconds()
        );
        let tempdir = std::env::temp_dir().join(unique);
        fs::create_dir_all(&tempdir).expect("create tempdir");
        let output = tempdir.join("mcp.json");
        fs::write(
            &output,
            json!({
                "servers": {
                    "amai": {
                        "command": "/tmp/run_mcp_stdio.sh",
                        "cwd": "/tmp",
                        "args": [],
                        "type": "stdio"
                    }
                }
            })
            .to_string(),
        )
        .expect("write config");
        let result = remove_vscode_mcp_config(&output, true).expect("remove config");
        assert!(result.removed);
        assert!(result.purged_file);
        assert!(!output.exists());
        let _ = fs::remove_dir_all(&tempdir);
    }
}
