use anyhow::{Context, Result, anyhow};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
struct DeploymentTargetsFile {
    targets: BTreeMap<String, DeploymentTarget>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DeploymentTarget {
    pub display_name: String,
    pub support_level: SupportLevel,
    pub summary: String,
    pub why_choose: Vec<String>,
    pub not_for: Vec<String>,
    pub requires_commands: Vec<String>,
    pub optional_commands: Vec<String>,
    pub next_step: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SupportLevel {
    Materialized,
    FoundationReady,
    Future,
}

#[derive(Debug)]
struct CommandStatus {
    command: String,
    found_at: Option<PathBuf>,
    required: bool,
}

#[derive(Debug)]
struct TargetPreflight {
    target_code: String,
    target: DeploymentTarget,
    commands: Vec<CommandStatus>,
}

pub fn print_targets(repo_root: &Path) -> Result<()> {
    let registry = load_targets(repo_root)?;
    println!("Amai deployment targets");
    println!();
    println!(
        "Это не список случайных идей, а канонические режимы развёртывания и проверки для Amai."
    );
    println!("Текущий базовый режим для обычного пользователя: local_docker.");
    println!();
    for (index, (code, target)) in ordered_targets(&registry.targets).into_iter().enumerate() {
        println!(
            "{}. {} ({}) — {}",
            index + 1,
            target.display_name,
            code,
            support_level_short(&target.support_level)
        );
        println!("   {}", target.summary);
    }
    Ok(())
}

pub fn print_target_explainer(repo_root: &Path, target_code: &str) -> Result<()> {
    let target = load_target(repo_root, target_code)?;
    println!("Amai deployment target");
    println!();
    println!("Режим: {} ({})", target.display_name, target_code);
    println!(
        "Статус в продукте: {}",
        support_level_title(&target.support_level)
    );
    println!();
    println!("Простыми словами:");
    println!("{}", support_level_explainer(&target.support_level));
    println!("Коротко: {}", target.summary);
    println!();
    println!("Когда этот режим выбирать:");
    for item in &target.why_choose {
        println!("- {}", item);
    }
    if !target.not_for.is_empty() {
        println!();
        println!("Когда этот режим лучше не выбирать:");
        for item in &target.not_for {
            println!("- {}", item);
        }
    }
    println!();
    println!("Следующий шаг:");
    println!("- {}", target.next_step);
    Ok(())
}

pub fn print_target_preflight(repo_root: &Path, target_code: &str) -> Result<()> {
    let report = target_preflight(repo_root, target_code)?;
    println!("Amai deployment target preflight");
    println!();
    println!(
        "Режим: {} ({})",
        report.target.display_name, report.target_code
    );
    println!(
        "Статус в продукте: {}",
        support_level_title(&report.target.support_level)
    );
    println!("Готовность этой машины: {}", readiness_title(&report));
    println!();
    println!("Простыми словами:");
    println!("{}", readiness_explainer(&report));
    println!("Коротко: {}", report.target.summary);
    println!();
    println!("Что проверили на этой машине:");
    for status in &report.commands {
        let label = if status.required {
            "обязательно"
        } else {
            "необязательно"
        };
        match &status.found_at {
            Some(path) => println!(
                "- {}: найдено ({}, {})",
                status.command,
                label,
                path.display()
            ),
            None => println!("- {}: не найдено ({})", status.command, label),
        }
    }
    let missing_required = missing_required(&report);
    if !missing_required.is_empty() {
        println!();
        println!("Что сейчас блокирует этот режим:");
        for item in missing_required {
            println!("- {}", item);
        }
    }
    println!();
    println!("Следующий шаг:");
    println!("- {}", report.target.next_step);
    Ok(())
}

fn target_preflight(repo_root: &Path, target_code: &str) -> Result<TargetPreflight> {
    let target = load_target(repo_root, target_code)?;
    let mut commands = Vec::new();
    for command in &target.requires_commands {
        commands.push(CommandStatus {
            command: command.clone(),
            found_at: find_command(command),
            required: true,
        });
    }
    for command in &target.optional_commands {
        commands.push(CommandStatus {
            command: command.clone(),
            found_at: find_command(command),
            required: false,
        });
    }
    Ok(TargetPreflight {
        target_code: target_code.to_string(),
        target,
        commands,
    })
}

fn missing_required(report: &TargetPreflight) -> Vec<String> {
    report
        .commands
        .iter()
        .filter(|item| item.required && item.found_at.is_none())
        .map(|item| format!("не найдено обязательное средство: {}", item.command))
        .collect()
}

fn readiness_title(report: &TargetPreflight) -> &'static str {
    if !missing_required(report).is_empty() {
        "пока не готово"
    } else {
        match report.target.support_level {
            SupportLevel::Materialized => "готово к работе",
            SupportLevel::FoundationReady => "задел уже готов",
            SupportLevel::Future => "пока только следующий слой",
        }
    }
}

fn readiness_explainer(report: &TargetPreflight) -> String {
    if !missing_required(report).is_empty() {
        return "На этой машине пока не хватает обязательных средств для этого режима. Сначала нужно доставить недостающие зависимости.".to_string();
    }
    match report.target.support_level {
        SupportLevel::Materialized => {
            "Этот режим уже реально поддержан в Amai и на этой машине есть базовые зависимости, чтобы им пользоваться.".to_string()
        }
        SupportLevel::FoundationReady => {
            "Этот режим ещё не является главным путём для обычного пользователя, но задел уже заложен: базовые зависимости на месте и следующий deployment-pass можно строить от этой точки.".to_string()
        }
        SupportLevel::Future => {
            "Этот режим пока зафиксирован как следующий слой эволюции. Машина может быть частично готова, но сам продукт ещё не обещает здесь полный finished-contour.".to_string()
        }
    }
}

fn support_level_short(level: &SupportLevel) -> &'static str {
    match level {
        SupportLevel::Materialized => "уже поддержано",
        SupportLevel::FoundationReady => "задел готов",
        SupportLevel::Future => "следующий слой",
    }
}

fn support_level_title(level: &SupportLevel) -> &'static str {
    match level {
        SupportLevel::Materialized => "уже поддержано",
        SupportLevel::FoundationReady => "задел уже заложен",
        SupportLevel::Future => "запланировано как следующий слой",
    }
}

fn support_level_explainer(level: &SupportLevel) -> &'static str {
    match level {
        SupportLevel::Materialized => {
            "Этот режим уже материализован в продукте и не является пустой бумажной идеей."
        }
        SupportLevel::FoundationReady => {
            "Этот режим ещё не должен подменять основной путь, но product foundation для него уже заложен."
        }
        SupportLevel::Future => {
            "Этот режим пока не обещается как готовый user-path и считается следующим этапом развития."
        }
    }
}

fn load_target(repo_root: &Path, target_code: &str) -> Result<DeploymentTarget> {
    let registry = load_targets(repo_root)?;
    registry
        .targets
        .get(target_code)
        .cloned()
        .ok_or_else(|| anyhow!("unknown deployment target: {target_code}"))
}

fn load_targets(repo_root: &Path) -> Result<DeploymentTargetsFile> {
    let path = targets_path(repo_root);
    let content = fs::read_to_string(&path)
        .with_context(|| format!("failed to read deployment targets {}", path.display()))?;
    toml::from_str(&content).context("failed to parse deployment targets")
}

fn targets_path(repo_root: &Path) -> PathBuf {
    repo_root.join("config/deployment_targets.toml")
}

fn ordered_targets(
    targets: &BTreeMap<String, DeploymentTarget>,
) -> Vec<(&String, &DeploymentTarget)> {
    let preferred = [
        "local_docker",
        "remote_ssh",
        "kubernetes_server",
        "windows_vm_lab",
    ];
    let mut ordered = Vec::new();
    for code in preferred {
        if let Some(target) = targets.get_key_value(code) {
            ordered.push(target);
        }
    }
    for item in targets {
        if !preferred.contains(&item.0.as_str()) {
            ordered.push(item);
        }
    }
    ordered
}

fn find_command(command: &str) -> Option<PathBuf> {
    let path_var = env::var_os("PATH")?;
    for dir in env::split_paths(&path_var) {
        let candidate = dir.join(command);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_materialized_target() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let target = load_target(repo_root, "local_docker").expect("target must load");
        assert_eq!(target.display_name, "Local Docker Baseline");
        assert!(matches!(target.support_level, SupportLevel::Materialized));
    }

    #[test]
    fn loads_future_target() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let target = load_target(repo_root, "windows_vm_lab").expect("target must load");
        assert!(matches!(target.support_level, SupportLevel::Materialized));
    }
}
