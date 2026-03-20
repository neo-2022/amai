use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};
use sysinfo::{Disks, System};

#[derive(Debug, Deserialize)]
struct DeploymentProfilesFile {
    profiles: std::collections::BTreeMap<String, DeploymentProfile>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DeploymentProfile {
    pub display_name: String,
    pub summary: String,
    pub suitable_for: Vec<String>,
    pub not_for: Vec<String>,
    pub minimum_cpu_logical: usize,
    pub minimum_memory_gib: f64,
    pub minimum_disk_gib: f64,
    pub recommended_cpu_logical: usize,
    pub recommended_memory_gib: f64,
    pub recommended_disk_gib: f64,
    pub supports_peak_benchmarks: bool,
    pub start_monitoring_by_default: bool,
    pub remote_mode_recommended: bool,
}

#[derive(Debug)]
pub struct PreflightReport {
    pub profile_code: String,
    pub profile: DeploymentProfile,
    pub host_logical_cpus: usize,
    pub host_total_memory_gib: f64,
    pub host_available_memory_gib: f64,
    pub host_available_disk_gib: f64,
    pub verdict: &'static str,
    pub unmet_minimums: Vec<String>,
    pub unmet_recommendations: Vec<String>,
}

pub fn load_profile(repo_root: &Path, profile_code: &str) -> Result<DeploymentProfile> {
    let path = profiles_path(repo_root);
    let content = fs::read_to_string(&path)
        .with_context(|| format!("failed to read deployment profiles {}", path.display()))?;
    let registry: DeploymentProfilesFile =
        toml::from_str(&content).context("failed to parse deployment profiles")?;
    registry
        .profiles
        .get(profile_code)
        .cloned()
        .ok_or_else(|| anyhow!("unknown deployment profile: {profile_code}"))
}

pub fn print_preflight(repo_root: &Path, profile_code: &str) -> Result<()> {
    let report = preflight_report(repo_root, profile_code)?;
    print_preflight_report(&report);
    Ok(())
}

pub fn print_preflight_report(report: &PreflightReport) {
    println!("Amai preflight");
    println!();
    println!(
        "Профиль: {} ({})",
        report.profile.display_name, report.profile_code
    );
    println!("Итог: {}", verdict_title(report.verdict));
    println!();
    println!("Простыми словами:");
    println!("{}", verdict_explainer(report));
    println!("Коротко о профиле: {}", report.profile.summary);
    println!();
    println!("Что проверили:");
    println!(
        "- CPU: {} логических потоков. Нужно минимум {}, комфортно от {}. {}",
        report.host_logical_cpus,
        report.profile.minimum_cpu_logical,
        report.profile.recommended_cpu_logical,
        resource_status_usize(
            report.host_logical_cpus,
            report.profile.minimum_cpu_logical,
            report.profile.recommended_cpu_logical
        )
    );
    println!(
        "- Память: {:.2} GiB всего, свободно сейчас {:.2} GiB. Нужно минимум {:.1} GiB, комфортно от {:.1} GiB. {}",
        report.host_total_memory_gib,
        report.host_available_memory_gib,
        report.profile.minimum_memory_gib,
        report.profile.recommended_memory_gib,
        resource_status_f64(
            report.host_total_memory_gib,
            report.profile.minimum_memory_gib,
            report.profile.recommended_memory_gib
        )
    );
    println!(
        "- Диск: свободно {:.2} GiB. Нужно минимум {:.1} GiB, комфортно от {:.1} GiB. {}",
        report.host_available_disk_gib,
        report.profile.minimum_disk_gib,
        report.profile.recommended_disk_gib,
        resource_status_f64(
            report.host_available_disk_gib,
            report.profile.minimum_disk_gib,
            report.profile.recommended_disk_gib
        )
    );
    println!();
    println!("Для чего этот режим подходит:");
    for item in &report.profile.suitable_for {
        println!("- {}", item);
    }
    if !report.profile.not_for.is_empty() {
        println!();
        println!("Когда лучше выбрать другой режим:");
        for item in &report.profile.not_for {
            println!("- {}", item);
        }
    }
    if !report.unmet_minimums.is_empty() {
        println!();
        println!("Что сейчас блокирует запуск в этом режиме:");
        for item in &report.unmet_minimums {
            println!("- {}", item);
        }
    }
    if !report.unmet_recommendations.is_empty() {
        println!();
        println!("Где есть риск, даже если запуск возможен:");
        for item in &report.unmet_recommendations {
            println!("- {}", item);
        }
    }
    println!();
    println!("Что делать дальше:");
    for item in next_steps(report) {
        println!("- {}", item);
    }
    println!();
    println!("Что эта машина реально потянет:");
    println!(
        "- Жёсткие proof и benchmark-контуры: {}",
        yes_no(report.profile.supports_peak_benchmarks)
    );
    println!(
        "- Monitoring можно включать сразу: {}",
        yes_no(report.profile.start_monitoring_by_default)
    );
    println!(
        "- Удалённый режим здесь уместен: {}",
        yes_no(report.profile.remote_mode_recommended)
    );
    println!(
        "- Подходящий профиль в этом запуске: {}",
        report.profile.display_name
    );
}

pub fn preflight_report(repo_root: &Path, profile_code: &str) -> Result<PreflightReport> {
    let profile = load_profile(repo_root, profile_code)?;
    let host = probe_host(repo_root)?;

    let mut unmet_minimums = Vec::new();
    let mut unmet_recommendations = Vec::new();

    if host.logical_cpus < profile.minimum_cpu_logical {
        unmet_minimums.push(format!(
            "logical CPU below minimum: {} < {}",
            host.logical_cpus, profile.minimum_cpu_logical
        ));
    } else if host.logical_cpus < profile.recommended_cpu_logical {
        unmet_recommendations.push(format!(
            "logical CPU below recommendation: {} < {}",
            host.logical_cpus, profile.recommended_cpu_logical
        ));
    }

    if host.total_memory_gib < profile.minimum_memory_gib {
        unmet_minimums.push(format!(
            "total memory below minimum: {:.2} < {:.1} GiB",
            host.total_memory_gib, profile.minimum_memory_gib
        ));
    } else if host.total_memory_gib < profile.recommended_memory_gib {
        unmet_recommendations.push(format!(
            "total memory below recommendation: {:.2} < {:.1} GiB",
            host.total_memory_gib, profile.recommended_memory_gib
        ));
    }

    if host.available_disk_gib < profile.minimum_disk_gib {
        unmet_minimums.push(format!(
            "available disk below minimum: {:.2} < {:.1} GiB",
            host.available_disk_gib, profile.minimum_disk_gib
        ));
    } else if host.available_disk_gib < profile.recommended_disk_gib {
        unmet_recommendations.push(format!(
            "available disk below recommendation: {:.2} < {:.1} GiB",
            host.available_disk_gib, profile.recommended_disk_gib
        ));
    }

    let verdict = if !unmet_minimums.is_empty() {
        "fail"
    } else if !unmet_recommendations.is_empty() {
        "warn"
    } else {
        "pass"
    };

    Ok(PreflightReport {
        profile_code: profile_code.to_string(),
        profile,
        host_logical_cpus: host.logical_cpus,
        host_total_memory_gib: host.total_memory_gib,
        host_available_memory_gib: host.available_memory_gib,
        host_available_disk_gib: host.available_disk_gib,
        verdict,
        unmet_minimums,
        unmet_recommendations,
    })
}

fn profiles_path(repo_root: &Path) -> PathBuf {
    repo_root.join("config/deployment_profiles.toml")
}

struct HostSnapshot {
    logical_cpus: usize,
    total_memory_gib: f64,
    available_memory_gib: f64,
    available_disk_gib: f64,
}

fn probe_host(repo_root: &Path) -> Result<HostSnapshot> {
    let mut system = System::new_all();
    system.refresh_memory();

    let disks = Disks::new_with_refreshed_list();
    let available_disk_bytes = match disk_available_for_path(&disks, repo_root) {
        Some(value) => value,
        None => bail!(
            "failed to detect available disk space for {}",
            repo_root.display()
        ),
    };

    Ok(HostSnapshot {
        logical_cpus: system.cpus().len(),
        total_memory_gib: bytes_to_gib(system.total_memory()),
        available_memory_gib: bytes_to_gib(system.available_memory()),
        available_disk_gib: bytes_to_gib(available_disk_bytes),
    })
}

fn disk_available_for_path(disks: &Disks, path: &Path) -> Option<u64> {
    let canonical = path.canonicalize().ok()?;
    disks
        .iter()
        .filter(|disk| canonical.starts_with(disk.mount_point()))
        .max_by_key(|disk| disk.mount_point().as_os_str().len())
        .map(|disk| disk.available_space())
}

fn bytes_to_gib(bytes: u64) -> f64 {
    bytes as f64 / 1024_f64 / 1024_f64 / 1024_f64
}

fn verdict_title(verdict: &str) -> &'static str {
    match verdict {
        "pass" => "машина подходит",
        "warn" => "машина подходит с оговорками",
        "fail" => "машина не подходит для этого режима",
        _ => "статус неизвестен",
    }
}

fn verdict_explainer(report: &PreflightReport) -> String {
    match report.verdict {
        "pass" => format!(
            "Эта машина уверенно подходит для профиля \"{}\". Можно рассчитывать на тот сценарий, для которого этот профиль задуман.",
            report.profile.display_name
        ),
        "warn" => format!(
            "Эта машина может работать в профиле \"{}\", но без запаса прочности. Базовый запуск возможен, однако часть тяжёлых сценариев лучше не обещать заранее.",
            report.profile.display_name
        ),
        "fail" => format!(
            "Эта машина слишком слабая для профиля \"{}\". В таком режиме лучше не продолжать установку, пока не уменьшите требования или не возьмёте более сильный хост.",
            report.profile.display_name
        ),
        _ => "Не удалось сформулировать понятный вывод.".to_string(),
    }
}

fn resource_status_usize(actual: usize, minimum: usize, recommended: usize) -> &'static str {
    if actual < minimum {
        "Этого мало даже для минимального сценария."
    } else if actual < recommended {
        "Минимум выполняется, но комфортного запаса нет."
    } else {
        "Есть нормальный запас."
    }
}

fn resource_status_f64(actual: f64, minimum: f64, recommended: f64) -> &'static str {
    if actual < minimum {
        "Этого мало даже для минимального сценария."
    } else if actual < recommended {
        "Минимум выполняется, но комфортного запаса нет."
    } else {
        "Есть нормальный запас."
    }
}

fn next_steps(report: &PreflightReport) -> Vec<String> {
    match report.verdict {
        "pass" => vec![
            format!("Машина подходит для профиля \"{}\".", report.profile_code),
            "Если хотите сравнить оба режима и выбрать между ними, запустите ./scripts/preflight.sh без параметров."
                .to_string(),
            format!(
                "Если хотите установить именно этот профиль сразу, используйте ./scripts/install_amai.sh --stack-profile {}.",
                report.profile_code
            ),
        ],
        "warn" => {
            let mut steps = vec![
                format!(
                    "Запуск возможен, но без запаса прочности в профиле \"{}\".",
                    report.profile_code
                ),
                "Если хотите сравнить оба режима и выбрать спокойнее, запустите ./scripts/preflight.sh без параметров."
                    .to_string(),
                format!(
                    "Если ограничения вас устраивают, установить именно этот профиль можно так: ./scripts/install_amai.sh --stack-profile {}.",
                    report.profile_code
                ),
            ];
            if report.profile_code == "default" {
                steps.push(
                    "Если хотите более лёгкий и дешёвый режим, сначала проверьте профиль \"lite_vps\"."
                        .to_string(),
                );
            }
            steps
        }
        "fail" => {
            let mut steps = vec![
                "Сначала устраните блокирующие ограничения по CPU, памяти или диску."
                    .to_string(),
                "Для быстрого сравнения доступных режимов запустите ./scripts/preflight.sh без параметров."
                    .to_string(),
            ];
            if report.profile_code == "default" {
                steps.push(
                    "Если вам нужен не полный локальный режим, а лёгкий удалённый режим, проверьте профиль \"lite_vps\"."
                        .to_string(),
                );
            }
            steps
        }
        _ => vec!["Перезапустите проверку.".to_string()],
    }
}

fn yes_no(value: bool) -> &'static str {
    if value { "да" } else { "нет" }
}

#[cfg(test)]
mod tests {
    use super::DeploymentProfile;

    #[test]
    fn profile_supports_plain_human_fields() {
        let profile = DeploymentProfile {
            display_name: "Lite VPS".to_string(),
            summary: "cheap remote smoke profile".to_string(),
            suitable_for: vec!["small smoke".to_string()],
            not_for: vec!["peak benchmark".to_string()],
            minimum_cpu_logical: 1,
            minimum_memory_gib: 2.0,
            minimum_disk_gib: 20.0,
            recommended_cpu_logical: 2,
            recommended_memory_gib: 4.0,
            recommended_disk_gib: 30.0,
            supports_peak_benchmarks: false,
            start_monitoring_by_default: false,
            remote_mode_recommended: true,
        };

        assert_eq!(profile.display_name, "Lite VPS");
        assert!(!profile.supports_peak_benchmarks);
        assert!(profile.remote_mode_recommended);
    }
}
