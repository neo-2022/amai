use crate::bootstrap;
use crate::cli::{BootstrapDisconnectArgs, BootstrapOnboardingArgs, McpConfigArgs};
use crate::config::AppConfig;
use crate::mcp;
use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::process::Command;

pub async fn run(args: &BootstrapOnboardingArgs) -> Result<()> {
    let repo_root = discover_repo_root(args.cwd.as_deref())?;
    ensure_local_config_files(&repo_root)?;
    dotenvy::from_path_override(repo_root.join(".env"))
        .context("failed to load generated .env for onboarding")?;

    check_dependency("docker", &["--version"]).await?;
    check_dependency("cargo", &["--version"]).await?;

    if !args.skip_stack {
        run_command(
            "docker compose up",
            command_in(
                &repo_root,
                "docker",
                ["compose", "up", "-d", "--remove-orphans"],
            ),
        )
        .await?;
        let cfg = AppConfig::from_env()?;
        bootstrap::bootstrap_stack(&cfg).await?;
    }

    if !args.skip_release_build {
        run_command(
            "cargo build --release",
            command_in(&repo_root, "cargo", ["build", "--release"]),
        )
        .await?;
    }

    let target = load_client_target(&repo_root, &args.client)?;
    let output = resolve_output_path(&repo_root, &target, args.output.as_ref())?;
    let backup = maybe_backup_user_global(&output, &target.install_scope)?;

    let config_args = McpConfigArgs {
        client: args.client.clone(),
        server_name: "amai".to_string(),
        launcher_platform: args.launcher_platform.clone(),
        command: None,
        cwd: Some(repo_root.clone()),
        output: Some(output.clone()),
    };
    mcp::write_client_config(&config_args)?;

    let release_binary = repo_root.join("target/release/amai");
    let release_ready = release_binary.is_file();

    println!("onboarding completed");
    println!("repo_root: {}", repo_root.display());
    println!("env_file: {}", repo_root.join(".env").display());
    println!("client: {}", args.client);
    println!("client_config: {}", output.display());
    println!(
        "client_config_mode: {}",
        install_scope_status(&target.install_scope)
    );
    println!("release_binary_ready: {release_ready}");
    if let Some(backup) = backup {
        println!("backup_file: {}", backup.display());
    }
    println!("next_step_1: open the repo in your client");
    println!("next_step_2: reload the client window or restart the client");
    println!("next_step_3: ask the client to call Amai through MCP");
    Ok(())
}

pub async fn disconnect(args: &BootstrapDisconnectArgs) -> Result<()> {
    let repo_root = discover_repo_root(args.cwd.as_deref())?;
    let target = load_client_target(&repo_root, &args.client)?;
    let output = resolve_output_path(&repo_root, &target, args.output.as_ref())?;
    let backup = maybe_backup_user_global(&output, &target.install_scope)?;

    let result = mcp::remove_client_config(
        &McpConfigArgs {
            client: args.client.clone(),
            server_name: "amai".to_string(),
            launcher_platform: "auto".to_string(),
            command: None,
            cwd: Some(repo_root),
            output: Some(output.clone()),
        },
        args.purge_empty_file,
    )?;

    println!("disconnect completed");
    println!("client: {}", args.client);
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
    if let Some(path) = explicit {
        return normalize_repo_root(path);
    }

    let cwd = std::env::current_dir().context("failed to resolve current working directory")?;
    for ancestor in cwd.ancestors() {
        if is_repo_root(ancestor) {
            return Ok(ancestor.to_path_buf());
        }
    }

    bail!("failed to discover Amai repo root; pass --cwd explicitly");
}

fn normalize_repo_root(path: &Path) -> Result<PathBuf> {
    let canonical = path
        .canonicalize()
        .with_context(|| format!("failed to resolve {}", path.display()))?;
    if !is_repo_root(&canonical) {
        bail!(
            "{} is not an Amai repo root (expected Cargo.toml, compose.yaml, scripts/run_mcp_stdio.sh)",
            canonical.display()
        );
    }
    Ok(canonical)
}

fn is_repo_root(path: &Path) -> bool {
    path.join("Cargo.toml").is_file()
        && path.join("compose.yaml").is_file()
        && path.join("scripts/run_mcp_stdio.sh").is_file()
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
    clients: BTreeMap<String, ClientTarget>,
}

#[derive(Debug, Clone, Deserialize)]
struct ClientTarget {
    default_output: String,
    install_scope: String,
}

fn load_client_target(repo_root: &Path, client: &str) -> Result<ClientTarget> {
    let manifest_path = repo_root.join("config/client_targets.toml");
    let content = fs::read_to_string(&manifest_path)
        .with_context(|| format!("failed to read {}", manifest_path.display()))?;
    let manifest: ClientTargetsManifest =
        toml::from_str(&content).context("failed to parse config/client_targets.toml")?;
    let client_key = client.trim().to_ascii_lowercase();
    manifest.clients.get(&client_key).cloned().ok_or_else(|| {
        anyhow!(
            "unsupported onboarding client target: {client_key}; register it in config/client_targets.toml"
        )
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
        "workspace_local" => "ready",
        "user_global" => "installed_in_user_scope",
        "manual_generated" => "generated_for_manual_import",
        _ => "generated",
    }
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
        env_keys, expand_target_template, install_scope_status, load_client_target,
        resolve_output_path,
    };
    use std::path::Path;

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
        let target = load_client_target(repo, "vscode").expect("vscode target must exist");
        assert_eq!(target.install_scope, "workspace_local");
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
        assert_eq!(install_scope_status("workspace_local"), "ready");
        assert_eq!(
            install_scope_status("user_global"),
            "installed_in_user_scope"
        );
        assert_eq!(
            install_scope_status("manual_generated"),
            "generated_for_manual_import"
        );
    }

    #[test]
    fn resolve_output_path_prefers_explicit_path() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
        let target = load_client_target(repo, "vscode").unwrap();
        let explicit = repo.join("custom/mcp.json");
        assert_eq!(
            resolve_output_path(repo, &target, Some(&explicit)).unwrap(),
            explicit
        );
    }
}
