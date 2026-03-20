use crate::bootstrap;
use crate::cli::{BootstrapOnboardingArgs, McpConfigArgs};
use crate::config::AppConfig;
use crate::mcp;
use anyhow::{Context, Result, anyhow, bail};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
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

    let output = resolve_output_path(&repo_root, &args.client, args.output.as_ref())?;
    let config_args = McpConfigArgs {
        client: args.client.clone(),
        server_name: "amai".to_string(),
        command: None,
        cwd: Some(repo_root.clone()),
        output: Some(output.clone()),
    };
    mcp::write_client_config(&config_args)?;

    let release_binary = repo_root.join("target/release/amai");
    let release_ready = release_binary.is_file();
    let config_mode = if client_config_is_workspace_local(&args.client) {
        "ready"
    } else {
        "generated_for_manual_import"
    };

    println!("onboarding completed");
    println!("repo_root: {}", repo_root.display());
    println!("env_file: {}", repo_root.join(".env").display());
    println!("client: {}", args.client);
    println!("client_config: {}", output.display());
    println!("client_config_mode: {config_mode}");
    println!("release_binary_ready: {release_ready}");
    println!("next_step_1: open the repo in your client");
    println!("next_step_2: reload the client window or restart the client");
    println!("next_step_3: ask the client to call Amai through MCP");
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

fn resolve_output_path(
    repo_root: &Path,
    client: &str,
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

    let client = client.trim().to_ascii_lowercase();
    let path = match client.as_str() {
        "vscode" => repo_root.join(".vscode/mcp.json"),
        "codex" => repo_root.join("tmp/onboarding/codex-mcp.toml"),
        "cursor" => repo_root.join("tmp/onboarding/cursor-mcp.json"),
        "claude-desktop" => repo_root.join("tmp/onboarding/claude-desktop-mcp.json"),
        "generic" => repo_root.join("tmp/onboarding/generic-mcp.json"),
        other => bail!(
            "unsupported onboarding client target: {other}; use vscode|cursor|claude-desktop|codex|generic"
        ),
    };
    Ok(path)
}

fn client_config_is_workspace_local(client: &str) -> bool {
    matches!(client.trim().to_ascii_lowercase().as_str(), "vscode")
}

#[cfg(test)]
mod tests {
    use super::{client_config_is_workspace_local, env_keys, resolve_output_path};
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
    fn resolves_default_output_paths() {
        let repo = Path::new("/tmp/amai");
        assert_eq!(
            resolve_output_path(repo, "vscode", None).unwrap(),
            repo.join(".vscode/mcp.json")
        );
        assert_eq!(
            resolve_output_path(repo, "codex", None).unwrap(),
            repo.join("tmp/onboarding/codex-mcp.toml")
        );
    }

    #[test]
    fn reports_workspace_local_clients() {
        assert!(client_config_is_workspace_local("vscode"));
        assert!(!client_config_is_workspace_local("cursor"));
    }
}
